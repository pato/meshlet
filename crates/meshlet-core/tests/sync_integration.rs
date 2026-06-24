use meshlet_core::loro::VersionVector;
use meshlet_core::model::{Bookmark, BookmarkId};
use meshlet_core::MeshletDb;

fn make_bm(url: &str, title: &str, tags: &[&str]) -> Bookmark {
    Bookmark {
        id: BookmarkId::new(),
        url: url.to_string(),
        title: title.to_string(),
        desc: "".into(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        flags: 0,
        created_at: meshlet_core::model::now_ts(),
        updated_at: meshlet_core::model::now_ts(),
    }
}

#[test]
fn test_two_clients_sync_converge() {
    let db_a = MeshletDb::open_in_memory().unwrap();
    let db_b = MeshletDb::open_in_memory().unwrap();

    let bm_a = make_bm("https://rust-lang.org", "Rust", &["lang"]);
    let bm_b = make_bm("https://loro.dev", "Loro", &["crdt"]);

    db_a.add_bookmark(&bm_a).unwrap();
    db_b.add_bookmark(&bm_b).unwrap();

    let updates_a = db_a.export_updates_since(&VersionVector::default()).unwrap();
    let updates_b = db_b.export_updates_since(&VersionVector::default()).unwrap();

    db_b.sync_import(&updates_a).unwrap();
    db_a.sync_import(&updates_b).unwrap();

    let list_a = db_a.list_from_mirror().unwrap();
    let list_b = db_b.list_from_mirror().unwrap();

    assert_eq!(list_a.len(), 2);
    assert_eq!(list_b.len(), 2);

    let urls_a: Vec<&str> = list_a.iter().map(|b| b.url.as_str()).collect();
    assert!(urls_a.contains(&"https://rust-lang.org"));
    assert!(urls_a.contains(&"https://loro.dev"));

    let urls_b: Vec<&str> = list_b.iter().map(|b| b.url.as_str()).collect();
    assert!(urls_b.contains(&"https://rust-lang.org"));
    assert!(urls_b.contains(&"https://loro.dev"));

    assert!(db_a.list_bookmarks().len() == 2);
    assert!(db_b.list_bookmarks().len() == 2);
}

#[test]
fn test_two_clients_same_url_dedup_on_sync() {
    let db_a = MeshletDb::open_in_memory().unwrap();
    let db_b = MeshletDb::open_in_memory().unwrap();

    let bm_a = make_bm("https://example.com", "Title A", &["a"]);
    let bm_b = make_bm("https://example.com", "Title B", &["b"]);

    db_a.add_bookmark(&bm_a).unwrap();
    db_b.add_bookmark(&bm_b).unwrap();

    let updates_a = db_a.export_updates_since(&VersionVector::default()).unwrap();
    let updates_b = db_b.export_updates_since(&VersionVector::default()).unwrap();

    db_b.sync_import(&updates_a).unwrap();
    db_a.sync_import(&updates_b).unwrap();

    let list_a = db_a.list_from_mirror().unwrap();
    let list_b = db_b.list_from_mirror().unwrap();

    assert_eq!(list_a.len(), 1);
    assert_eq!(list_b.len(), 1);
    assert_eq!(list_a[0].url, "https://example.com");
    assert!(list_a[0].tags.contains("a"));
    assert!(list_a[0].tags.contains("b"));
}

#[test]
fn test_sync_preserves_mirror_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path_a = dir.path().join("a.db");
    let path_b = dir.path().join("b.db");

    let bm_a = make_bm("https://keep-a.com", "Keep A", &[]);
    let bm_b = make_bm("https://keep-b.com", "Keep B", &[]);

    let del_id: BookmarkId;

    {
        let db_a = MeshletDb::open(&path_a).unwrap();
        let db_b = MeshletDb::open(&path_b).unwrap();

        db_a.add_bookmark(&bm_a).unwrap();
        db_b.add_bookmark(&bm_b).unwrap();

        let del = make_bm("https://delete-me.com", "Delete Me", &[]);
        del_id = del.id.clone();
        db_b.add_bookmark(&del).unwrap();

        let updates_a = db_a.export_updates_since(&VersionVector::default()).unwrap();

        db_b.sync_import(&updates_a).unwrap();
        db_b.delete_bookmark(&del_id).unwrap();

        let updates_b = db_b.export_updates_since(&VersionVector::default()).unwrap();
        db_a.sync_import(&updates_b).unwrap();
    }

    {
        let db_a = MeshletDb::open(&path_a).unwrap();
        let db_b = MeshletDb::open(&path_b).unwrap();

        let list_a = db_a.list_from_mirror().unwrap();
        let list_b = db_b.list_from_mirror().unwrap();

        assert!(!list_a.iter().any(|b| b.id == del_id), "deleted bookmark should not appear in A's mirror");
        assert!(!list_b.iter().any(|b| b.id == del_id), "deleted bookmark should not appear in B's mirror");

        assert!(list_a.iter().any(|b| b.url == "https://keep-a.com"));
        assert!(list_a.iter().any(|b| b.url == "https://keep-b.com"));
    }
}
