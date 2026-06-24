pub mod error;
pub mod model;
pub mod doc;
pub mod store;
pub mod search;
pub mod fetch;
pub mod reconcile;

pub use rusqlite;
pub use loro;

use std::path::Path;

use crate::error::MeshletError;
use error::Result;
use model::{Bookmark, BookmarkId, BookmarkPatch};
use store::Store;

#[derive(Debug, Clone, Default)]
pub struct SyncSummary {
    pub merged_duplicates: usize,
}

pub struct MeshletDb {
    inner: doc::LoroStore,
    db: Store,
}

impl MeshletDb {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Store::open(path)?;

        let inner = if let Some(snapshot) = db.load_snapshot()? {
            doc::LoroStore::from_snapshot(&snapshot)?
        } else {
            let peer_id = ulid::Ulid::new().to_string();
            db.set_meta_string("peer_id", &peer_id)?;
            doc::LoroStore::new()
        };

        let this = Self { inner, db };
        this.rebuild_mirror()?;

        Ok(this)
    }

    pub fn open_in_memory() -> Result<Self> {
        let db = Store::open_in_memory()?;
        let inner = doc::LoroStore::new();
        Ok(Self { inner, db })
    }

    pub fn add_bookmark(&self, b: &Bookmark) -> Result<()> {
        self.inner.add_bookmark(b)?;
        self.db.upsert_bookmark(b)?;
        self.save_snapshot()?;
        Ok(())
    }

    pub fn update_bookmark(&self, id: &BookmarkId, patch: &BookmarkPatch) -> Result<()> {
        self.inner.update_bookmark(id, patch)?;
        if let Some(updated) = self.inner.get_bookmark(id) {
            self.db.upsert_bookmark(&updated)?;
        }
        self.save_snapshot()?;
        Ok(())
    }

    pub fn delete_bookmark(&self, id: &BookmarkId) -> Result<()> {
        self.inner.delete_bookmark(id)?;
        self.db.delete_bookmark_mirror(id.as_str())?;
        self.save_snapshot()?;
        Ok(())
    }

    pub fn add_tags(&self, id: &BookmarkId, tags: &[String]) -> Result<()> {
        self.inner.add_tags(id, tags)?;
        if let Some(updated) = self.inner.get_bookmark(id) {
            self.db.upsert_bookmark(&updated)?;
        }
        self.save_snapshot()?;
        Ok(())
    }

    pub fn remove_tags(&self, id: &BookmarkId, tags: &[String]) -> Result<()> {
        self.inner.remove_tags(id, tags)?;
        if let Some(updated) = self.inner.get_bookmark(id) {
            self.db.upsert_bookmark(&updated)?;
        }
        self.save_snapshot()?;
        Ok(())
    }

    pub fn get_bookmark(&self, id: &BookmarkId) -> Option<Bookmark> {
        self.inner.get_bookmark(id)
    }

    pub fn list_bookmarks(&self) -> Vec<Bookmark> {
        self.inner.list_bookmarks()
    }

    pub fn import(&self, data: &[u8]) -> Result<()> {
        self.inner.import(data)?;
        reconcile::reconcile(&self.inner)?;
        self.rebuild_mirror()?;
        self.save_snapshot()?;
        Ok(())
    }

    pub fn export_snapshot(&self) -> Result<Vec<u8>> {
        self.inner.export_snapshot()
    }

    pub fn search_keywords(
        &self,
        keywords: &[String],
        deep: bool,
        all_match: bool,
    ) -> Result<Vec<Bookmark>> {
        search::search_keywords(self.db.connection(), keywords, deep, all_match)
    }

    pub fn search_by_tags(&self, tags: &[String]) -> Result<Vec<Bookmark>> {
        search::search_by_tags(self.db.connection(), tags)
    }

    pub fn list_from_mirror(&self) -> Result<Vec<Bookmark>> {
        search::list_all(self.db.connection())
    }

    pub fn inner_connection(&self) -> &rusqlite::Connection {
        self.db.connection()
    }

    pub fn oplog_vv(&self) -> loro::VersionVector {
        self.inner.oplog_vv()
    }

    pub fn export_updates_since(&self, vv: &loro::VersionVector) -> Result<Vec<u8>> {
        self.inner.export_updates_since(vv)
    }

    pub fn sync_import(&self, data: &[u8]) -> Result<SyncSummary> {
        self.inner.import(data)?;
        let merged = reconcile::reconcile(&self.inner)?;
        self.rebuild_mirror()?;
        self.save_snapshot()?;
        Ok(SyncSummary {
            merged_duplicates: merged,
        })
    }

    pub fn compact_change_store(&self) {
        self.inner.compact_change_store();
    }

    pub fn save_last_server_vv(&self, vv: &loro::VersionVector) -> Result<()> {
        let data = serde_json::to_vec(vv)
            .map_err(|e| MeshletError::SerializationError(e.to_string()))?;
        self.db.set_meta("last_server_vv", &data)
    }

    pub fn load_last_server_vv(&self) -> Result<Option<loro::VersionVector>> {
        match self.db.get_meta("last_server_vv")? {
            Some(data) => Ok(Some(serde_json::from_slice(&data)
                .map_err(|e| MeshletError::SerializationError(e.to_string()))?)),
            None => Ok(None),
        }
    }

    fn rebuild_mirror(&self) -> Result<()> {
        self.db.connection().execute("DELETE FROM bookmark_tags", [])?;
        self.db.connection().execute("DELETE FROM bookmarks", [])?;
        let bookmarks = self.inner.list_bookmarks();
        for b in &bookmarks {
            self.db.upsert_bookmark(b)?;
        }
        Ok(())
    }
    fn save_snapshot(&self) -> Result<()> {
        let snapshot = self.inner.export_snapshot()?;
        let vv = serde_json::to_vec(&self.inner.oplog_vv())
            .map_err(|e| error::MeshletError::SerializationError(e.to_string()))?;
        self.db.save_snapshot(&snapshot, &vv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mirror_rebuilt_on_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");

        let b = Bookmark {
            id: BookmarkId::new(),
            url: "https://example.com".into(),
            title: "Example".into(),
            desc: "A test bookmark".into(),
            tags: {
                let mut t = std::collections::BTreeSet::new();
                t.insert("test".into());
                t.insert("example".into());
                t
            },
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };

        {
            let db = MeshletDb::open(&path).unwrap();
            db.add_bookmark(&b).unwrap();
        }

        {
            let db = MeshletDb::open(&path).unwrap();
            let loaded = db.get_bookmark(&b.id).unwrap();
            assert_eq!(loaded.url, "https://example.com");
            assert_eq!(loaded.title, "Example");
            assert_eq!(loaded.desc, "A test bookmark");
            assert!(loaded.tags.contains("test"));
            assert!(loaded.tags.contains("example"));
        }
    }

    #[test]
    fn test_search_from_mirror() {
        let db = MeshletDb::open_in_memory().unwrap();

        let b1 = Bookmark {
            id: BookmarkId::new(),
            url: "https://rust-lang.org".into(),
            title: "Rust Programming Language".into(),
            desc: "A systems programming language".into(),
            tags: {
                let mut t = std::collections::BTreeSet::new();
                t.insert("rust".into());
                t
            },
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };

        let b2 = Bookmark {
            id: BookmarkId::new(),
            url: "https://loro.dev".into(),
            title: "Loro CRDT".into(),
            desc: "CRDT framework".into(),
            tags: {
                let mut t = std::collections::BTreeSet::new();
                t.insert("crdt".into());
                t.insert("rust".into());
                t
            },
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };

        db.add_bookmark(&b1).unwrap();
        db.add_bookmark(&b2).unwrap();

        let results = db.search_keywords(&["crdt".into()], false, false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://loro.dev");

        let results = db.search_keywords(&["rust".into()], false, false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://rust-lang.org");

        let results = db.search_by_tags(&["crdt".into()]).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://loro.dev");
    }

    #[test]
    fn test_delete_persists_across_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");

        let b = Bookmark {
            id: BookmarkId::new(),
            url: "https://example.com".into(),
            title: "Example".into(),
            desc: "".into(),
            tags: std::collections::BTreeSet::new(),
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };

        {
            let db = MeshletDb::open(&path).unwrap();
            db.add_bookmark(&b).unwrap();
            assert!(db.get_bookmark(&b.id).is_some());
            db.delete_bookmark(&b.id).unwrap();
            assert!(db.get_bookmark(&b.id).is_none());
        }

        {
            let db = MeshletDb::open(&path).unwrap();
            assert!(db.get_bookmark(&b.id).is_none());
            assert_eq!(db.list_bookmarks().len(), 0);
        }
    }

    #[test]
    fn test_update_reflected_in_mirror() {
        let db = MeshletDb::open_in_memory().unwrap();

        let b = Bookmark {
            id: BookmarkId::new(),
            url: "https://old.example.com".into(),
            title: "Old Title".into(),
            desc: "Old desc".into(),
            tags: {
                let mut t = std::collections::BTreeSet::new();
                t.insert("initial".into());
                t
            },
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };
        db.add_bookmark(&b).unwrap();

        let patch = BookmarkPatch {
            url: Some("https://new.example.com".into()),
            title: Some("New Title".into()),
            desc: None,
            flags: None,
        };
        db.update_bookmark(&b.id, &patch).unwrap();

        let mirror_results = db
            .search_keywords(&["New Title".into()], false, false)
            .unwrap();
        assert_eq!(mirror_results.len(), 1);
        assert_eq!(mirror_results[0].url, "https://new.example.com");
        assert_eq!(mirror_results[0].title, "New Title");
        assert_eq!(mirror_results[0].desc, "Old desc");
    }

    #[test]
    fn test_tag_sync_in_mirror() {
        let db = MeshletDb::open_in_memory().unwrap();

        let b = Bookmark {
            id: BookmarkId::new(),
            url: "https://example.com".into(),
            title: "Example".into(),
            desc: "".into(),
            tags: std::collections::BTreeSet::new(),
            flags: 0,
            created_at: 0,
            updated_at: 0,
        };
        db.add_bookmark(&b).unwrap();

        db.add_tags(&b.id, &["rust".into(), "crdt".into()])
            .unwrap();
        db.remove_tags(&b.id, &["crdt".into()]).unwrap();

        let results = db.search_by_tags(&["rust".into()]).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].tags.contains("rust"));
        assert!(!results[0].tags.contains("crdt"));
    }
}