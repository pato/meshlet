use std::time::Duration;

use reqwest::blocking::Client;
use scraper::{Html, Selector};

#[derive(Debug, Clone)]
pub struct FetchResult {
    pub final_url: String,
    pub title: Option<String>,
    pub desc: Option<String>,
    pub tags: Vec<String>,
    pub status: u16,
    pub is_mime: bool,
    pub bad: bool,
}

pub fn fetch_bookmark_data(url: &str) -> FetchResult {
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => {
            return FetchResult {
                final_url: url.to_string(),
                title: None,
                desc: None,
                tags: vec![],
                status: 0,
                is_mime: false,
                bad: true,
            };
        }
    };

    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return FetchResult {
            final_url: url.to_string(),
            title: None,
            desc: None,
            tags: vec![],
            status: 0,
            is_mime: false,
            bad: true,
        };
    }

    let client = match Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(
            "Mozilla/5.0 (compatible; Meshlet/0.1; +https://github.com/meshlet)",
        )
        .redirect(reqwest::redirect::Policy::limited(5))
        .danger_accept_invalid_certs(false)
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return FetchResult {
                final_url: url.to_string(),
                title: None,
                desc: None,
                tags: vec![],
                status: 0,
                is_mime: false,
                bad: true,
            };
        }
    };

    match client.head(url).send() {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let final_url = resp.url().to_string();
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            let is_html = content_type.contains("text/html")
                || content_type.contains("application/xhtml+xml");

            if !is_html {
                return FetchResult {
                    final_url,
                    title: None,
                    desc: None,
                    tags: vec![],
                    status,
                    is_mime: content_type.contains("application/")
                        || content_type.contains("image/")
                        || content_type.contains("audio/")
                        || content_type.contains("video/"),
                    bad: status >= 400,
                };
            }

            if status >= 400 {
                return FetchResult {
                    final_url,
                    title: None,
                    desc: None,
                    tags: vec![],
                    status,
                    is_mime: false,
                    bad: true,
                };
            }

            match client.get(url).send() {
                Ok(get_resp) => {
                    let final_url = get_resp.url().to_string();
                    let status = get_resp.status().as_u16();

                    if status >= 400 {
                        return FetchResult {
                            final_url,
                            title: None,
                            desc: None,
                            tags: vec![],
                            status,
                            is_mime: false,
                            bad: true,
                        };
                    }

                    let body = match get_resp.text() {
                        Ok(t) => t,
                        Err(_) => {
                            return FetchResult {
                                final_url,
                                title: None,
                                desc: None,
                                tags: vec![],
                                status,
                                is_mime: false,
                                bad: true,
                            };
                        }
                    };

                    let document = Html::parse_document(&body);
                    let title = extract_title(&document);
                    let desc = extract_meta(&document, "description");
                    let tags = extract_keywords(&document);

                    FetchResult {
                        final_url,
                        title,
                        desc,
                        tags,
                        status,
                        is_mime: false,
                        bad: false,
                    }
                }
                Err(_) => FetchResult {
                    final_url: url.to_string(),
                    title: None,
                    desc: None,
                    tags: vec![],
                    status: 0,
                    is_mime: false,
                    bad: true,
                },
            }
        }
        Err(_) => FetchResult {
            final_url: url.to_string(),
            title: None,
            desc: None,
            tags: vec![],
            status: 0,
            is_mime: false,
            bad: true,
        },
    }
}

fn extract_title(document: &Html) -> Option<String> {
    let selector = Selector::parse("title").ok()?;
    document
        .select(&selector)
        .next()
        .map(|el| el.text().collect::<Vec<_>>().join(""))
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
}

fn extract_meta(document: &Html, name: &str) -> Option<String> {
    let selector =
        Selector::parse(&format!("meta[name=\"{}\"]", name)).ok()?;
    document
        .select(&selector)
        .next()
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn extract_keywords(document: &Html) -> Vec<String> {
    let selector = Selector::parse("meta[name=\"keywords\"]").ok();
    let content = selector.and_then(|sel| {
        document
            .select(&sel)
            .next()
            .and_then(|el| el.value().attr("content"))
            .map(|s| s.to_string())
    });

    match content {
        Some(s) => s
            .split(',')
            .map(|kw| kw.trim().to_string())
            .filter(|kw| !kw.is_empty())
            .collect(),
        None => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static HTML_SIMPLE: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>Simple Test Page</title>
    <meta name="description" content="A simple test page for testing">
    <meta name="keywords" content="test, simple, rust">
</head>
<body><p>Hello world</p></body>
</html>"#;

    static HTML_NO_DESC: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>No Description Page</title>
</head>
<body><p>No meta tags here</p></body>
</html>"#;

    static HTML_EMPTY: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title></title>
</head>
<body></body>
</html>"#;

    static HTML_UNICODE: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>Café & Crème — Spécial</title>
    <meta name="description" content="Testing unicode café characters">
    <meta name="keywords" content="café, crème, ünicode">
</head>
<body></body>
</html>"#;

    static HTML_TITLE_WITH_WHITESPACE: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>
        Multi-line
        Title
    </title>
    <meta name="description" content="   ">
    <meta name="keywords" content="tag1, , tag2, ">
</head>
<body></body>
</html>"#;

    #[test]
    fn test_extract_simple_title() {
        let doc = Html::parse_document(HTML_SIMPLE);
        assert_eq!(extract_title(&doc), Some("Simple Test Page".into()));
    }

    #[test]
    fn test_extract_description() {
        let doc = Html::parse_document(HTML_SIMPLE);
        assert_eq!(
            extract_meta(&doc, "description"),
            Some("A simple test page for testing".into())
        );
    }

    #[test]
    fn test_extract_keywords() {
        let doc = Html::parse_document(HTML_SIMPLE);
        let tags = extract_keywords(&doc);
        assert_eq!(tags, vec!["test", "simple", "rust"]);
    }

    #[test]
    fn test_no_description() {
        let doc = Html::parse_document(HTML_NO_DESC);
        assert_eq!(extract_title(&doc), Some("No Description Page".into()));
        assert_eq!(extract_meta(&doc, "description"), None);
        assert!(extract_keywords(&doc).is_empty());
    }

    #[test]
    fn test_empty_title() {
        let doc = Html::parse_document(HTML_EMPTY);
        assert_eq!(extract_title(&doc), None);
    }

    #[test]
    fn test_unicode_handling() {
        let doc = Html::parse_document(HTML_UNICODE);
        assert_eq!(extract_title(&doc), Some("Café & Crème — Spécial".into()));
        assert_eq!(
            extract_meta(&doc, "description"),
            Some("Testing unicode café characters".into())
        );
        assert_eq!(
            extract_keywords(&doc),
            vec!["café", "crème", "ünicode"]
        );
    }

    #[test]
    fn test_whitespace_handling() {
        let doc = Html::parse_document(HTML_TITLE_WITH_WHITESPACE);
        assert_eq!(extract_title(&doc), Some("Multi-line Title".into()));
        assert_eq!(extract_meta(&doc, "description"), None);
        assert_eq!(extract_keywords(&doc), vec!["tag1", "tag2"]);
    }

    #[test]
    fn test_bad_url() {
        let result = fetch_bookmark_data("not-a-valid-url!!!");
        assert!(result.bad);
    }

    #[test]
    fn test_non_http_url() {
        let result = fetch_bookmark_data("ftp://example.com/file");
        assert!(result.bad);
    }
}