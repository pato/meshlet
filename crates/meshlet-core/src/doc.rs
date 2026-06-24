use std::borrow::Cow;
use std::collections::BTreeSet;

use loro::{Container, ExportMode, LoroDoc, LoroMap, LoroValue, ValueOrContainer};

use crate::error::{MeshletError, Result};
use crate::model::{Bookmark, BookmarkId, BookmarkPatch};

const BOOKMARKS: &str = "bookmarks";
const TAGS: &str = "tags";
const FIELD_URL: &str = "url";
const FIELD_TITLE: &str = "title";
const FIELD_DESC: &str = "desc";
const FIELD_IMMUTABLE: &str = "immutable_title";
const FIELD_CREATED_AT: &str = "created_at";
const FIELD_UPDATED_AT: &str = "updated_at";

pub struct LoroStore {
    doc: LoroDoc,
}

impl LoroStore {
    pub fn new() -> Self {
        let doc = LoroDoc::new();
        doc.set_record_timestamp(true);
        doc.get_map(BOOKMARKS);
        doc.commit();
        Self { doc }
    }
}

impl Default for LoroStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LoroStore {
    pub fn from_snapshot(data: &[u8]) -> Result<Self> {
        let doc =
            LoroDoc::from_snapshot(data).map_err(MeshletError::LoroError)?;
        doc.set_record_timestamp(true);
        Ok(Self { doc })
    }

    pub fn export_snapshot(&self) -> Result<Vec<u8>> {
        self.doc
            .export(ExportMode::Snapshot)
            .map_err(|e| MeshletError::LoroError(e.into()))
    }

    pub fn export_updates_since(
        &self,
        vv: &loro::VersionVector,
    ) -> Result<Vec<u8>> {
        self.doc
            .export(ExportMode::Updates {
                from: Cow::Borrowed(vv),
            })
            .map_err(|e| MeshletError::LoroError(e.into()))
    }

    pub fn import(&self, data: &[u8]) -> Result<ImportStatus> {
        self.doc
            .import(data)
            .map_err(MeshletError::LoroError)?;
        Ok(ImportStatus { success: true })
    }

    pub fn oplog_vv(&self) -> loro::VersionVector {
        self.doc.oplog_vv().clone()
    }

    pub fn state_vv(&self) -> loro::VersionVector {
        self.doc.state_vv().clone()
    }

    pub fn add_bookmark(&self, b: &Bookmark) -> Result<()> {
        let bookmarks = self.bookmarks_map()?;
        let child = bookmarks.ensure_mergeable_map(b.id.as_str())?;

        let now = crate::model::now_ts();
        let created = if b.created_at > 0 { b.created_at } else { now };

        child.insert(FIELD_URL, b.url.as_str())?;
        child.insert(FIELD_TITLE, b.title.as_str())?;
        child.insert(FIELD_DESC, b.desc.as_str())?;
        child.insert(FIELD_IMMUTABLE, b.flags & 0x01 != 0)?;
        child.insert(FIELD_CREATED_AT, created)?;
        child.insert(FIELD_UPDATED_AT, now)?;

        let tags_map = child.ensure_mergeable_map(TAGS)?;
        for tag in &b.tags {
            tags_map.insert(tag.as_str(), true)?;
        }

        self.doc.commit();
        Ok(())
    }

    pub fn update_bookmark(&self, id: &BookmarkId, patch: &BookmarkPatch) -> Result<()> {
        let bookmarks = self.bookmarks_map()?;
        let child = self
            .get_child_map(&bookmarks, id.as_str())?
            .ok_or_else(|| MeshletError::BookmarkNotFound(id.to_string()))?;

        if let Some(ref url) = patch.url {
            child.insert(FIELD_URL, url.as_str())?;
        }
        if let Some(ref title) = patch.title {
            child.insert(FIELD_TITLE, title.as_str())?;
        }
        if let Some(ref desc) = patch.desc {
            child.insert(FIELD_DESC, desc.as_str())?;
        }
        if let Some(flags) = patch.flags {
            child.insert(FIELD_IMMUTABLE, flags & 0x01 != 0)?;
        }

        child.insert(FIELD_UPDATED_AT, crate::model::now_ts())?;

        self.doc.commit();
        Ok(())
    }

    pub fn delete_bookmark(&self, id: &BookmarkId) -> Result<()> {
        let bookmarks = self.bookmarks_map()?;
        bookmarks.delete(id.as_str())?;
        self.doc.commit();
        Ok(())
    }

    pub fn add_tags(&self, id: &BookmarkId, tags: &[String]) -> Result<()> {
        let bookmarks = self.bookmarks_map()?;
        let child = self
            .get_child_map(&bookmarks, id.as_str())?
            .ok_or_else(|| MeshletError::BookmarkNotFound(id.to_string()))?;

        let tags_map = child.ensure_mergeable_map(TAGS)?;

        for tag in tags {
            tags_map.insert(tag.as_str(), true)?;
        }

        child.insert(FIELD_UPDATED_AT, crate::model::now_ts())?;
        self.doc.commit();
        Ok(())
    }

    pub fn remove_tags(&self, id: &BookmarkId, tags: &[String]) -> Result<()> {
        let bookmarks = self.bookmarks_map()?;
        let child = self
            .get_child_map(&bookmarks, id.as_str())?
            .ok_or_else(|| MeshletError::BookmarkNotFound(id.to_string()))?;

        let tags_map = child.ensure_mergeable_map(TAGS)?;

        for tag in tags {
            tags_map.delete(tag.as_str())?;
        }

        child.insert(FIELD_UPDATED_AT, crate::model::now_ts())?;
        self.doc.commit();
        Ok(())
    }

    pub fn get_bookmark(&self, id: &BookmarkId) -> Option<Bookmark> {
        let bookmarks = self.bookmarks_map().ok()?;
        let child = self.get_child_map(&bookmarks, id.as_str()).ok()??;
        Some(self.read_bookmark(id, &child))
    }

    pub fn list_bookmarks(&self) -> Vec<Bookmark> {
        let bookmarks = self.bookmarks_map().unwrap();
        let mut results = Vec::new();
        bookmarks.for_each(|key, value| {
            if let ValueOrContainer::Container(Container::Map(child)) = value {
                let id = BookmarkId(key.to_string());
                results.push(self.read_bookmark(&id, &child));
            }
        });
        results
    }

    pub fn compact_change_store(&self) {
        self.doc.compact_change_store();
    }

    fn bookmarks_map(&self) -> Result<LoroMap> {
        Ok(self.doc.get_map(BOOKMARKS))
    }

    fn get_child_map(&self, parent: &LoroMap, key: &str) -> Result<Option<LoroMap>> {
        match parent.get(key) {
            Some(ValueOrContainer::Container(Container::Map(m))) => Ok(Some(m)),
            None => Ok(None),
            Some(_) => Err(MeshletError::LoroError(loro::LoroError::internal(
                format!("expected map at key '{}', found different type", key),
            ))),
        }
    }

    fn read_bookmark(&self, id: &BookmarkId, child: &LoroMap) -> Bookmark {
        let url = read_string_field(child, FIELD_URL).unwrap_or_default();
        let title = read_string_field(child, FIELD_TITLE).unwrap_or_default();
        let desc = read_string_field(child, FIELD_DESC).unwrap_or_default();
        let immutable = read_bool_field(child, FIELD_IMMUTABLE).unwrap_or(false);
        let flags: i64 = if immutable { 0x01 } else { 0 };
        let created_at = read_i64_field(child, FIELD_CREATED_AT).unwrap_or(0);
        let updated_at = read_i64_field(child, FIELD_UPDATED_AT).unwrap_or(0);

        let tags: BTreeSet<String> = {
            let mut set = BTreeSet::new();
            child.for_each(|key, value| {
                if key == TAGS
                    && let ValueOrContainer::Container(Container::Map(tags_map)) = value
                {
                    tags_map.for_each(|tag_key, _| {
                        set.insert(tag_key.to_string());
                    });
                }
            });
            set
        };

        Bookmark {
            id: id.clone(),
            url,
            title,
            desc,
            tags,
            flags,
            created_at,
            updated_at,
        }
    }
}

fn read_string_field(map: &LoroMap, key: &str) -> Option<String> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn read_bool_field(map: &LoroMap, key: &str) -> Option<bool> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::Bool(b)) => Some(b),
        _ => None,
    }
}

fn read_i64_field(map: &LoroMap, key: &str) -> Option<i64> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::I64(n)) => Some(n),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct ImportStatus {
    pub success: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip_single_bookmark() {
        let store = LoroStore::new();
        let mut tags = BTreeSet::new();
        tags.insert("rust".into());
        tags.insert("crdt".into());

        let b = Bookmark {
            id: BookmarkId::new(),
            url: "https://loro.dev".into(),
            title: "Loro CRDT".into(),
            desc: "High-performance CRDT framework".into(),
            tags: tags.clone(),
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };

        store.add_bookmark(&b).unwrap();

        let snapshot = store.export_snapshot().unwrap();
        let store2 = LoroStore::from_snapshot(&snapshot).unwrap();

        let loaded = store2.get_bookmark(&b.id).unwrap();
        assert_eq!(loaded.url, "https://loro.dev");
        assert_eq!(loaded.title, "Loro CRDT");
        assert_eq!(loaded.desc, "High-performance CRDT framework");
        assert_eq!(loaded.tags, tags);
    }

    #[test]
    fn test_round_trip_multiple_bookmarks_with_tags() {
        let store = LoroStore::new();

        for i in 0..5 {
            let mut tags = BTreeSet::new();
            if i % 2 == 0 {
                tags.insert("even".into());
            }
            if i % 3 == 0 {
                tags.insert("three".into());
            }
            tags.insert(format!("index-{}", i));

            let b = Bookmark {
                id: BookmarkId::new(),
                url: format!("https://example.com/{}", i),
                title: format!("Page {}", i),
                desc: format!("Description {}", i),
                tags,
                flags: 0,
                created_at: 0,
                updated_at: 0,
            };

            store.add_bookmark(&b).unwrap();
        }

        let snapshot = store.export_snapshot().unwrap();
        let store2 = LoroStore::from_snapshot(&snapshot).unwrap();

        let list = store2.list_bookmarks();
        assert_eq!(list.len(), 5);
    }

    #[test]
    fn test_tag_operations() {
        let store = LoroStore::new();
        let b = Bookmark {
            id: BookmarkId::new(),
            url: "https://example.com".into(),
            title: "Example".into(),
            desc: "".into(),
            tags: BTreeSet::new(),
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };
        store.add_bookmark(&b).unwrap();

        store
            .add_tags(&b.id, &["rust".into(), "crdt".into()])
            .unwrap();
        let loaded = store.get_bookmark(&b.id).unwrap();
        assert!(loaded.tags.contains("rust"));
        assert!(loaded.tags.contains("crdt"));

        store.remove_tags(&b.id, &["crdt".into()]).unwrap();
        let loaded = store.get_bookmark(&b.id).unwrap();
        assert!(loaded.tags.contains("rust"));
        assert!(!loaded.tags.contains("crdt"));
    }

    #[test]
    fn test_update_bookmark() {
        let store = LoroStore::new();
        let b = Bookmark {
            id: BookmarkId::new(),
            url: "https://example.com".into(),
            title: "Old Title".into(),
            desc: "Old Desc".into(),
            tags: BTreeSet::new(),
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };
        store.add_bookmark(&b).unwrap();

        let patch = BookmarkPatch {
            url: Some("https://new-url.com".into()),
            title: Some("New Title".into()),
            desc: None,
            flags: Some(0x01),
        };
        store.update_bookmark(&b.id, &patch).unwrap();

        let loaded = store.get_bookmark(&b.id).unwrap();
        assert_eq!(loaded.url, "https://new-url.com");
        assert_eq!(loaded.title, "New Title");
        assert_eq!(loaded.desc, "Old Desc");
        assert_eq!(loaded.flags, 0x01);
    }

    #[test]
    fn test_delete_bookmark() {
        let store = LoroStore::new();
        let b = Bookmark {
            id: BookmarkId::new(),
            url: "https://example.com".into(),
            title: "Example".into(),
            desc: "".into(),
            tags: BTreeSet::new(),
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };
        store.add_bookmark(&b).unwrap();
        assert!(store.get_bookmark(&b.id).is_some());

        store.delete_bookmark(&b.id).unwrap();
        assert!(store.get_bookmark(&b.id).is_none());
    }

    #[test]
    fn test_duplicate_tag_concurrent_add() {
        let store = LoroStore::new();
        let b = Bookmark {
            id: BookmarkId::new(),
            url: "https://example.com".into(),
            title: "Example".into(),
            desc: "".into(),
            tags: BTreeSet::new(),
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };
        store.add_bookmark(&b).unwrap();

        store.add_tags(&b.id, &["rust".into()]).unwrap();
        store.add_tags(&b.id, &["rust".into()]).unwrap();

        let loaded = store.get_bookmark(&b.id).unwrap();
        assert_eq!(loaded.tags.iter().filter(|t| *t == "rust").count(), 1);
    }

    #[test]
    fn test_two_peers_concurrent_adds_converge() {
        let store_a = LoroStore::new();
        let store_b = LoroStore::new();

        let id_a = BookmarkId::new();
        let id_b = BookmarkId::new();

        let mut tags_a = BTreeSet::new();
        tags_a.insert("rust".into());
        let bm_a = Bookmark {
            id: id_a.clone(),
            url: "https://rust-lang.org".into(),
            title: "Rust".into(),
            desc: "".into(),
            tags: tags_a,
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };

        let mut tags_b = BTreeSet::new();
        tags_b.insert("crdt".into());
        let bm_b = Bookmark {
            id: id_b.clone(),
            url: "https://loro.dev".into(),
            title: "Loro".into(),
            desc: "".into(),
            tags: tags_b,
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };

        store_a.add_bookmark(&bm_a).unwrap();
        store_b.add_bookmark(&bm_b).unwrap();

        let snapshot_a = store_a.export_snapshot().unwrap();
        let snapshot_b = store_b.export_snapshot().unwrap();

        store_a.import(&snapshot_b).unwrap();
        store_b.import(&snapshot_a).unwrap();

        let list_a = store_a.list_bookmarks();
        let list_b = store_b.list_bookmarks();
        assert_eq!(list_a.len(), 2);
        assert_eq!(list_b.len(), 2);

        assert!(store_a.get_bookmark(&id_a).is_some());
        assert!(store_a.get_bookmark(&id_b).is_some());
        assert!(store_b.get_bookmark(&id_a).is_some());
        assert!(store_b.get_bookmark(&id_b).is_some());
    }

    #[test]
    fn test_two_peers_concurrent_same_bookmark_converges() {
        let store_a = LoroStore::new();
        let store_b = LoroStore::new();

        let shared_id = BookmarkId::new();

        let bm_a = Bookmark {
            id: shared_id.clone(),
            url: "https://example.com".into(),
            title: "From A".into(),
            desc: "Desc A".into(),
            tags: {
                let mut t = BTreeSet::new();
                t.insert("a-tag".into());
                t
            },
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };

        let bm_b = Bookmark {
            id: shared_id.clone(),
            url: "https://example.com".into(),
            title: "From B".into(),
            desc: "Desc B".into(),
            tags: {
                let mut t = BTreeSet::new();
                t.insert("b-tag".into());
                t
            },
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };

        store_a.add_bookmark(&bm_a).unwrap();
        store_b.add_bookmark(&bm_b).unwrap();

        let snapshot_a = store_a.export_snapshot().unwrap();
        store_b.import(&snapshot_a).unwrap();

        let snapshot_b = store_b.export_snapshot().unwrap();
        store_a.import(&snapshot_b).unwrap();

        let a_bookmark = store_a.get_bookmark(&shared_id).unwrap();
        let b_bookmark = store_b.get_bookmark(&shared_id).unwrap();

        assert_eq!(a_bookmark.url, b_bookmark.url);
        assert_eq!(a_bookmark.tags.len(), b_bookmark.tags.len());
        assert_eq!(a_bookmark.tags, b_bookmark.tags);
    }
}