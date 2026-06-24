use std::collections::HashMap;

use url::Url;

use crate::error::Result;
use crate::model::{Bookmark, BookmarkPatch};
use crate::doc::LoroStore;

pub fn reconcile(store: &LoroStore) -> Result<usize> {
    let bookmarks = store.list_bookmarks();
    let mut by_url: HashMap<String, Vec<Bookmark>> = HashMap::new();

    for bm in bookmarks {
        let normalized = normalize_url(&bm.url);
        by_url.entry(normalized).or_default().push(bm);
    }

    let mut merged = 0;

    for (_normalized, group) in by_url {
        if group.len() <= 1 {
            continue;
        }

        let mut sorted = group;
        sorted.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });

        let winner = &sorted[0];

        for loser in &sorted[1..] {
            let mut patch = BookmarkPatch::default();

            if winner.title.is_empty() && !loser.title.is_empty() {
                patch.title = Some(loser.title.clone());
            }
            if winner.desc.is_empty() && !loser.desc.is_empty() {
                patch.desc = Some(loser.desc.clone());
            }
            if winner.url.is_empty() && !loser.url.is_empty() {
                patch.url = Some(loser.url.clone());
            }

            if patch.url.is_some() || patch.title.is_some() || patch.desc.is_some() {
                let _ = store.update_bookmark(&winner.id, &patch);
            }

            let new_tags: Vec<String> = loser
                .tags
                .difference(&winner.tags)
                .cloned()
                .collect();
            if !new_tags.is_empty() {
                let _ = store.add_tags(&winner.id, &new_tags);
            }

            let _ = store.delete_bookmark(&loser.id);
            merged += 1;
        }
    }

    Ok(merged)
}

fn normalize_url(raw: &str) -> String {
    let normalized = raw.trim().to_lowercase();

    if let Ok(mut parsed) = Url::parse(&normalized) {
        parsed.set_fragment(None);
        let mut url_str = parsed.to_string();
        if url_str.ends_with('/') {
            url_str.pop();
        }
        url_str
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::LoroStore;
    use crate::model::{Bookmark, BookmarkId};

    fn make_bookmark(id: &BookmarkId, url: &str, title: &str, tags: &[&str], created_at: i64) -> Bookmark {
        Bookmark {
            id: id.clone(),
            url: url.to_string(),
            title: title.to_string(),
            desc: "".into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            flags: 0,
            created_at,
            updated_at: created_at,
        }
    }

    #[test]
    fn test_normalize_url_trailing_slash() {
        assert_eq!(normalize_url("https://example.com/"), "https://example.com");
    }

    #[test]
    fn test_normalize_url_fragment() {
        assert_eq!(
            normalize_url("https://example.com/page#section"),
            "https://example.com/page"
        );
    }

    #[test]
    fn test_normalize_url_case() {
        let a = normalize_url("HTTPS://EXAMPLE.COM/Path");
        let b = normalize_url("https://example.com/path");
        assert_eq!(a, b);
    }

    #[test]
    fn test_reconcile_merges_duplicates() {
        let store = LoroStore::new();
        let id1 = BookmarkId::new();
        let id2 = BookmarkId::new();

        store
            .add_bookmark(&make_bookmark(
                &id1,
                "https://example.com/",
                "Winner",
                &["a", "b"],
                1000,
            ))
            .unwrap();
        store
            .add_bookmark(&make_bookmark(
                &id2,
                "https://example.com",
                "Loser",
                &["b", "c"],
                2000,
            ))
            .unwrap();

        let merged = reconcile(&store).unwrap();
        assert_eq!(merged, 1);

        let remaining = store.list_bookmarks();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].title, "Winner");
        assert!(remaining[0].tags.contains("a"));
        assert!(remaining[0].tags.contains("b"));
        assert!(remaining[0].tags.contains("c"));
        assert_eq!(remaining[0].url, "https://example.com/");
    }

    #[test]
    fn test_reconcile_no_duplicates() {
        let store = LoroStore::new();

        store
            .add_bookmark(&make_bookmark(
                &BookmarkId::new(),
                "https://example.com/a",
                "A",
                &[],
                1000,
            ))
            .unwrap();
        store
            .add_bookmark(&make_bookmark(
                &BookmarkId::new(),
                "https://example.com/b",
                "B",
                &[],
                2000,
            ))
            .unwrap();

        let merged = reconcile(&store).unwrap();
        assert_eq!(merged, 0);
        assert_eq!(store.list_bookmarks().len(), 2);
    }
}