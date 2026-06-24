use std::collections::BTreeSet;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use meshlet_core::model::{Bookmark, BookmarkId};
use meshlet_core::MeshletDb;
use meshlet_proto::messages::{SyncRequest, SyncResponse};
use meshlet_server::{app_router, AppState, ServerDoc};
use tempfile::TempDir;
use tokio::sync::Mutex;
use tower::ServiceExt;

struct TestServer {
    router: axum::Router,
    _dir: TempDir,
}

impl TestServer {
    fn new(token: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let server_doc = ServerDoc::load_or_create(dir.path());
        let state = Arc::new(AppState {
            doc: Mutex::new(server_doc),
            token: token.map(String::from),
            data_dir: dir.path().to_path_buf(),
        });
        Self {
            router: app_router(state),
            _dir: dir,
        }
    }
}

async fn sync_once(router: &axum::Router, db: &MeshletDb, token: Option<&str>) {
    let last_vv = db.load_last_server_vv().unwrap();
    let client_updates = match &last_vv {
        Some(vv) => db.export_updates_since(vv).unwrap(),
        None => db
            .export_updates_since(&loro::VersionVector::default())
            .unwrap(),
    };
    let client_vv = db.oplog_vv();
    let request = SyncRequest::new(&client_vv, &client_updates);
    let body_json = serde_json::to_string(&request).unwrap();

    let mut builder = Request::builder()
        .method("POST")
        .uri("/sync")
        .header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {}", t));
    }
    let response = router
        .clone()
        .oneshot(builder.body(Body::from(body_json)).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let sync_response: SyncResponse = serde_json::from_slice(&bytes).unwrap();

    let server_updates = sync_response.updates().unwrap();
    let server_vv = sync_response.server_vv().unwrap();
    if !server_updates.is_empty() {
        db.sync_import(&server_updates).unwrap();
    }
    db.save_last_server_vv(&server_vv).unwrap();
}

fn make_bookmark(url: &str, title: &str, tags: &[&str]) -> Bookmark {
    let now = meshlet_core::model::now_ts();
    Bookmark {
        id: BookmarkId::new(),
        url: url.into(),
        title: title.into(),
        desc: String::new(),
        tags: tags.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(),
        flags: 0,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn two_clients_converge_via_server() {
    let server = TestServer::new(None);
    let client_a = MeshletDb::open_in_memory().unwrap();
    let client_b = MeshletDb::open_in_memory().unwrap();

    client_a
        .add_bookmark(&make_bookmark("https://rust-lang.org", "Rust", &["lang"]))
        .unwrap();
    client_b
        .add_bookmark(&make_bookmark("https://loro.dev", "Loro", &["crdt"]))
        .unwrap();

    for _ in 0..3 {
        sync_once(&server.router, &client_a, None).await;
        sync_once(&server.router, &client_b, None).await;
    }

    let a_list = client_a.list_from_mirror().unwrap();
    let b_list = client_b.list_from_mirror().unwrap();
    let a_urls: Vec<&str> = a_list.iter().map(|b| b.url.as_str()).collect();
    let b_urls: Vec<&str> = b_list.iter().map(|b| b.url.as_str()).collect();

    assert!(a_urls.contains(&"https://rust-lang.org"));
    assert!(a_urls.contains(&"https://loro.dev"));
    assert!(b_urls.contains(&"https://rust-lang.org"));
    assert!(b_urls.contains(&"https://loro.dev"));
    assert_eq!(a_urls.len(), 2);
    assert_eq!(b_urls.len(), 2);
}

#[tokio::test]
async fn dedup_merges_same_url() {
    let server = TestServer::new(None);
    let client_a = MeshletDb::open_in_memory().unwrap();
    let client_b = MeshletDb::open_in_memory().unwrap();

    client_a
        .add_bookmark(&make_bookmark("https://example.com/", "From A", &["a-tag"]))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    client_b
        .add_bookmark(&make_bookmark("https://example.com", "From B", &["b-tag"]))
        .unwrap();

    for _ in 0..3 {
        sync_once(&server.router, &client_a, None).await;
        sync_once(&server.router, &client_b, None).await;
    }

    let a_list = client_a.list_from_mirror().unwrap();
    let b_list = client_b.list_from_mirror().unwrap();
    assert_eq!(a_list.len(), 1, "client A should have one entry");
    assert_eq!(b_list.len(), 1, "client B should have one entry");

    let a = &a_list[0];
    let b = &b_list[0];
    assert_eq!(
        normalize(&a.url),
        normalize(&b.url),
        "both clients should keep the same URL"
    );
    assert!(
        a.tags.contains("a-tag") && a.tags.contains("b-tag"),
        "tags should be unioned on A: {:?}",
        a.tags
    );
    assert!(
        b.tags.contains("a-tag") && b.tags.contains("b-tag"),
        "tags should be unioned on B: {:?}",
        b.tags
    );
}

fn normalize(url: &str) -> String {
    let s = url.trim().to_lowercase();
    s.trim_end_matches('/').to_string()
}

#[tokio::test]
async fn unauthorized_token_rejected() {
    let server = TestServer::new(Some("secret-token"));

    let body = serde_json::to_string(&SyncRequest::new(
        &loro::VersionVector::default(),
        &[],
    ))
    .unwrap();

    let no_auth = send(&server.router, None, &body).await;
    assert_eq!(no_auth.status(), StatusCode::UNAUTHORIZED);

    let wrong = send(&server.router, Some("wrong"), &body).await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    let ok = send(&server.router, Some("secret-token"), &body).await;
    assert_eq!(ok.status(), StatusCode::OK);
}

async fn send(router: &axum::Router, token: Option<&str>, body: &str) -> axum::http::Response<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/sync")
        .header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {}", t));
    }
    router
        .clone()
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
