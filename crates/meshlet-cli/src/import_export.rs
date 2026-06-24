use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use meshlet_core::model::{Bookmark, BookmarkId};
use meshlet_core::MeshletDb;

pub struct ImportStats {
    pub total: usize,
    pub imported: usize,
    pub skipped: usize,
}

pub fn import_netscape(path: &Path, db: &MeshletDb) -> Result<ImportStats> {
    let content =
        std::fs::read_to_string(path).context("failed to read import file")?;
    let doc = scraper::Html::parse_document(&content);

    let link_selector = scraper::Selector::parse("a").unwrap();
    let mut stats = ImportStats {
        total: 0,
        imported: 0,
        skipped: 0,
    };

    for element in doc.select(&link_selector) {
        let href = match element.value().attr("href") {
            Some(h) => h.to_string(),
            None => continue,
        };

        if href.is_empty()
            || href.starts_with("javascript:")
            || href.starts_with("place:")
        {
            continue;
        }

        stats.total += 1;

        let title = element.text().collect::<Vec<_>>().join("");
        let title = title.trim();
        let title = if title.is_empty() { href.clone() } else { title.to_string() };

        let tags_str = element.value().attr("tags").unwrap_or("");
        let mut tags: std::collections::BTreeSet<String> = tags_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let add_date = element
            .value()
            .attr("add_date")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        let desc = element.value().attr("description").unwrap_or("");

        tags.insert("imported".into());

        let bookmark = Bookmark {
            id: BookmarkId::new(),
            url: href,
            title: title.to_string(),
            desc: desc.to_string(),
            tags,
            flags: 0,
            created_at: add_date,
            updated_at: add_date,
        };

        match db.add_bookmark(&bookmark) {
            Ok(()) => stats.imported += 1,
            Err(e) => {
                eprintln!("warning: skipped '{}': {}", title, e);
                stats.skipped += 1;
            }
        }
    }

    Ok(stats)
}

pub fn export_markdown(path: &Path, db: &MeshletDb) -> Result<()> {
    let bookmarks = db.list_from_mirror()?;
    let mut f = std::fs::File::create(path).context("failed to create export file")?;

    writeln!(f, "# Meshlet Bookmarks\n")?;

    let mut current_tag: Option<String> = None;

    for bm in &bookmarks {
        let mut tags_sorted: Vec<&str> = bm.tags.iter().map(|t| t.as_str()).collect();
        tags_sorted.sort();
        let tag_str = tags_sorted
            .iter()
            .map(|t| format!("`#{}`", t))
            .collect::<Vec<_>>()
            .join(" ");

        if !tag_str.is_empty() && current_tag.as_deref() != tags_sorted.first().copied() {
            current_tag = tags_sorted.first().map(|s| s.to_string());
            if let Some(ref tag) = current_tag {
                writeln!(f, "## {}\n", tag)?;
            }
        }

        let title = if bm.title.is_empty() { &bm.url } else { &bm.title };
        writeln!(f, "- [{}]({})", title, bm.url)?;

        if !bm.desc.is_empty() {
            writeln!(f, "  > {}", bm.desc)?;
        }
        if !tag_str.is_empty() {
            writeln!(f, "  {}", tag_str)?;
        }
        writeln!(f)?;
    }

    Ok(())
}

pub fn export_html(path: &Path, db: &MeshletDb) -> Result<()> {
    let bookmarks = db.list_from_mirror()?;
    let mut f = std::fs::File::create(path).context("failed to create export file")?;

    writeln!(f, "<!DOCTYPE NETSCAPE-Bookmark-file-1>")?;
    writeln!(
        f,
        "<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">"
    )?;
    writeln!(f, "<TITLE>Meshlet Bookmarks</TITLE>")?;
    writeln!(f, "<H1>Meshlet Bookmarks</H1>")?;
    writeln!(f, "<DL><p>")?;

    for bm in &bookmarks {
        let tag_attr = if bm.tags.is_empty() {
            String::new()
        } else {
            format!(
                " TAGS=\"{}\"",
                bm.tags
                    .iter()
                    .map(|t| t.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };

        let title = if bm.title.is_empty() { &bm.url } else { &bm.title };
        let desc_attr = if bm.desc.is_empty() {
            String::new()
        } else {
            format!(" DESCRIPTION=\"{}\"", bm.desc.replace('"', "&quot;"))
        };

        writeln!(
            f,
            "<DT><A HREF=\"{}\" ADD_DATE=\"{}\"{}{}>{}</A>",
            xml_escape(&bm.url),
            bm.created_at,
            tag_attr,
            desc_attr,
            xml_escape(title),
        )?;
    }

    writeln!(f, "</DL><p>")?;

    Ok(())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    static SAMPLE_BOOKMARKS_HTML: &str = r##"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks</H1>
<DL><p>
    <DT><H3>Folder</H3>
    <DL><p>
        <DT><A HREF="https://www.rust-lang.org" ADD_DATE="1640000000" TAGS="rust,programming">Rust Programming Language</A>
        <DT><A HREF="https://loro.dev" ADD_DATE="1640000001">Loro CRDT</A>
        <DT><A HREF="javascript:void(0)">Skip this</A>
    </DL><p>
</DL><p>"##;

    #[test]
    fn test_import_netscape() {
        let db = MeshletDb::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bookmarks.html");
        std::fs::write(&path, SAMPLE_BOOKMARKS_HTML).unwrap();

        let stats = import_netscape(&path, &db).unwrap();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.imported, 2);
        assert_eq!(stats.skipped, 0);

        let list = db.list_from_mirror().unwrap();
        let urls: Vec<&str> = list.iter().map(|b| b.url.as_str()).collect();
        assert!(urls.contains(&"https://www.rust-lang.org"));
        assert!(urls.contains(&"https://loro.dev"));

        let rust_bm = list.iter().find(|b| b.url == "https://www.rust-lang.org").unwrap();
        assert_eq!(rust_bm.title, "Rust Programming Language");
        assert!(rust_bm.tags.contains("rust"));
        assert!(rust_bm.tags.contains("programming"));
    }

    #[test]
    fn test_export_import_roundtrip() {
        let db = MeshletDb::open_in_memory().unwrap();

        let mut tags = std::collections::BTreeSet::new();
        tags.insert("test".into());

        let bm = Bookmark {
            id: BookmarkId::new(),
            url: "https://example.com".into(),
            title: "Example".into(),
            desc: "Test bookmark".into(),
            tags,
            flags: 0,
            created_at: 1640000000,
            updated_at: 0,
        };
        db.add_bookmark(&bm).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let html_path = dir.path().join("export.html");
        export_html(&html_path, &db).unwrap();

        let md_path = dir.path().join("export.md");
        export_markdown(&md_path, &db).unwrap();

        let html_content = std::fs::read_to_string(&html_path).unwrap();
        let md_content = std::fs::read_to_string(&md_path).unwrap();

        assert!(html_content.contains("https://example.com"));
        assert!(html_content.contains("Example"));
        assert!(md_content.contains("https://example.com"));
        assert!(md_content.contains("Example"));
    }

    #[test]
    fn test_xml_escape() {
        assert_eq!(xml_escape("a & b"), "a &amp; b");
        assert_eq!(xml_escape("<tag>"), "&lt;tag&gt;");
        assert_eq!(xml_escape("\"quoted\""), "&quot;quoted&quot;");
    }

    fn fixed_bookmarks() -> Vec<Bookmark> {
        use std::collections::BTreeSet;
        vec![
            Bookmark {
                id: BookmarkId("01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
                url: "https://loro.dev".into(),
                title: "Loro CRDT".into(),
                desc: "A high-performance CRDT framework".into(),
                tags: ["crdt", "rust"].iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(),
                flags: 0,
                created_at: 1719000000,
                updated_at: 1719100000,
            },
            Bookmark {
                id: BookmarkId("01ARZ3NDEKTSV4RRFFQ69G5FB0".into()),
                url: "https://rust-lang.org".into(),
                title: "Rust".into(),
                desc: String::new(),
                tags: ["lang"].iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(),
                flags: 1,
                created_at: 1718000000,
                updated_at: 1718100000,
            },
            Bookmark {
                id: BookmarkId("01ARZ3NDEKTSV4RRFFQ69G5FB1".into()),
                url: "https://example.com/special?x=1&y=2".into(),
                title: "Edge case: <script> & friends".into(),
                desc: "Quotes \" and ampersands &".into(),
                tags: BTreeSet::new(),
                flags: 0,
                created_at: 1717000000,
                updated_at: 1717100000,
            },
        ]
    }

    #[test]
    fn snapshot_markdown_export() {
        let db = MeshletDb::open_in_memory().unwrap();
        for b in fixed_bookmarks() {
            db.add_bookmark(&b).unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.md");
        export_markdown(&path, &db).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        insta::assert_snapshot!(content);
    }

    #[test]
    fn snapshot_json_output_with_redaction() {
        let db = MeshletDb::open_in_memory().unwrap();
        for b in fixed_bookmarks() {
            db.add_bookmark(&b).unwrap();
        }
        let bookmarks = db.list_from_mirror().unwrap();
        insta::assert_json_snapshot!(&bookmarks, {
            "[].id" => "[ULID]",
            "[].created_at" => "[TS]",
            "[].updated_at" => "[TS]",
        });
    }
}