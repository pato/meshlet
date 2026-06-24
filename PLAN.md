# Meshlet — Architecture Plan

## Overview

Meshlet is a personal link storage system (advanced bookmark manager) built in Rust, designed as a simpler re-implementation of [buku](https://github.com/jarun/buku) with first-class support for syncing between computers using CRDTs (Conflict-free Replicated Data Types).

### Key Features

- Store bookmarks with auto-fetched title, tags, and description
- Fully functional offline (local-first)
- Multi-device sync via a dedicated server
- CRDT-based conflict resolution (no merge conflicts)
- CLI interface (like buku)

### Explicit non-goals (v1)

- Browser extension / web UI (leave hooks, don't build)
- Multi-user accounts / sharing between *different people* (single-user, multi-device)
- Full-text indexing of bookmarked page contents (store metadata only)

---

## 1. Project Structure (Cargo Workspace)

```
meshlet/
├── Cargo.toml                       # [workspace], resolver = "2"
├── PLAN.md                          # this document
└── crates/
    ├── meshlet-core/                # lib: model, loro doc, sqlite, fetcher, search
    │   └── src/
    │       ├── lib.rs
    │       ├── model.rs             # Bookmark struct, BookmarkId (ULID)
    │       ├── doc.rs               # LoroDoc wrapper: load/save snapshot, CRUD, subscribe
    │       ├── store.rs             # SQLite: snapshot blob + queryable mirror tables
    │       ├── mirror.rs            # subscribe_root → upsert/delete SQL rows
    │       ├── fetch.rs             # reqwest + scraper: title/desc/keywords
    │       ├── search.rs            # SQL search by keyword/tag/regex (mirror)
    │       ├── reconcile.rs         # URL deduplication on import
    │       └── error.rs
    ├── meshlet-proto/               # shared sync wire types (serde) — client & server both depend on it
    │   └── src/
    │       ├── lib.rs
    │       └── messages.rs          # SyncRequest / SyncResponse, VV encoding
    ├── meshlet-cli/                 # the binary (à la buku): clap + $EDITOR
    │   └── src/
    │       ├── main.rs
    │       ├── args.rs              # clap derive
    │       ├── editor.rs            # tempfile + $EDITOR flow
    │       └── import_export.rs     # HTML/MD/JSON
    └── meshlet-server/              # sync relay binary (M6)
        └── src/
            ├── main.rs
            ├── doc.rs               # server's own LoroDoc + persistence
            └── transport.rs         # axum HTTP handler, token auth, TLS
```

### Rationale for 4 Crates

- **meshlet-core**: Data model + CRDT + SQLite + search + fetch + reconciliation. The `store.rs`/`mirror.rs`/`search.rs` modules form the persistence layer inside core — splitting them out adds friction without benefit at this scale.
- **meshlet-proto**: Shared sync wire types (`SyncRequest`, `SyncResponse`, VV encoding). Depends only on `serde` (+ `loro` for VV types). This lets `meshlet-server` depend on a lightweight proto crate instead of pulling in all of `core` (SQLite, scraper, etc.).
- **meshlet-cli**: CLI binary. Depends on `core` + `proto`.
- **meshlet-server**: Server binary. Depends on `core` (for the loro doc wrapper) + `proto`.

---

## 2. Data Model & CRDT Schema

### 2.1 Bookmark Entity

```rust
pub struct Bookmark {
    pub id:              BookmarkId,  // ULID — time-sortable, no coordination needed
    pub url:             String,      // the URL being bookmarked
    pub title:           String,      // page title (auto-fetched or manual)
    pub desc:            String,      // description (auto-fetched or manual)
    pub tags:            Set<String>, // set of tags
    pub flags:           i64,         // bitmask: 0x01 = immutable title
    pub created_at:      i64,         // Unix timestamp (from loro ChangeMeta)
    pub updated_at:      i64,         // Unix timestamp (from loro ChangeMeta)
}
```

### 2.2 CRDT Structure (in LoroDoc)

All bookmarks live in a **single `LoroDoc`** with a root `LoroMap` named `"bookmarks"`.

Each bookmark is keyed by `BookmarkId` (ULID string) and stored as a **child `LoroMap`**.
Child maps are created via **`ensure_mergeable_map`** so that two clients independently creating the same ULID get **deterministic container IDs** and converge instead of conflicting.

```
LoroMap("bookmarks") {
    "<ULID>": LoroMap {                    // created via ensure_mergeable_map
        "url":              LoroValue::String,   // LWW per field
        "title":            LoroValue::String,   // plain string, NOT LoroText
        "desc":             LoroValue::String,   // plain string, NOT LoroText
        "immutable_title":  LoroValue::Bool,
        "tags":             LoroMap {            // set semantics: tag → true
            "rust":  true,                       //   add    = insert key
            "crdt":  true,                       //   remove = delete key
        },
    }
}
```

**Key design notes:**
- **Title and description are plain `LoroValue::String`** (LWW), not `LoroText`. Per-character collaborative editing of descriptions is overkill for a bookmark manager and costs more space.
- **Tags use `LoroMap{tag → true}`** for true CRDT set semantics: concurrent adds of the same tag merge to one entry (no duplicates); deletions propagate correctly through causal ordering. See §2.3 for the detailed rationale.
- **`created_at`/`updated_at` come from loro's built-in timestamp tracking** (`doc.set_record_timestamp(true)`). Every `ChangeMeta` carries a Unix timestamp automatically — we derive timestamps from the op log when reading, not from LWW map fields. The first change to a bookmark's map is `created_at`; the latest change is `updated_at`.
- **Deletion** = removing the key from the root `bookmarks` map. This is a proper CRDT operation with causal propagation (see §2.5). No soft-delete flag needed.

#### Why a Single LoroDoc?

| Approach | Pros | Cons |
|----------|------|------|
| **Single LoroDoc** (Map of bookmarks) | One version vector to track; simple sync; Loro's Map CRDT handles concurrent adds naturally | Document grows monotonically (tombstones accumulate) |
| One LoroDoc per bookmark | Smaller individual docs; easier to GC | N version vectors to track; complex sync protocol |

For a personal bookmark manager (10K-100K entries over years), a single LoroDoc is the clear winner. The monotonic growth from tombstones is negligible at this scale.

#### Why ULIDs Instead of Sequential IDs or UUIDs?

Sequential integers conflict across offline peers (two peers assign the same ID to different bookmarks). UUIDs work but are 36-char random strings with no inherent ordering.

**ULID** (26 characters, e.g. `01ARZ3NDEKTSV4RRFFQ69G5FAV`) is the sweet spot:
- **Time-sortable**: the first 10 chars encode a millisecond timestamp, so lexicographic sort = chronological sort.
- **No coordination**: the random component ensures uniqueness across peers without a central authority.
- **Compact**: 26 chars vs UUID's 36, nice for display.

The local SQLite mirror maps ULIDs to sequential display indices for the buku-like `1. Title [tags]` output.

#### Tags: `LoroMap{tag → true}` (set semantics)

We use a **`LoroMap`** with `tag → true` entries per bookmark. This is the correct CRDT primitive for an unordered set of tags:

| Scenario | LoroMap result | LoroList result (hypothetical) |
|----------|---------------|-------------------------------|
| Two peers concurrently add the **same** tag `"rust"` | Single entry `{rust: true}` ✅ | Duplicate `["rust", "rust"]` ❌ |
| Peer A adds `"rust"`, Peer B adds `"go"` concurrently | Both present ✅ | Both present (tie-break on order) ✅ |
| Peer A deletes `"python"`, Peer B adds `"rust"` concurrently | `python` gone, `rust` present ✅ | Same ✅ |
| Peer A adds `"rust"`, Peer B deletes `"rust"` concurrently | Deterministic tie-break ✅ | Order-dependent 🤷 |

The critical difference is the first row: `LoroMap` keys are inherently unique, so concurrent adds of the same tag converge to one entry. `LoroList` allows duplicates and would require application-level deduplication.

The SQL mirror has a separate `bookmark_tags` table for fast `GROUP BY`/`LIKE` queries, populated from the loro map keys on each change.

#### `ensure_mergeable_map` — Why It Matters

When two peers independently create the same bookmark (same ULID), each creates a child `LoroMap`. Without `ensure_mergeable_map`, loro would assign **different container IDs** to these two maps — they'd be treated as separate entries instead of converging. `ensure_mergeable_map` ensures the container ID is deterministically derived from `(parent, key, type)`, so both peers' lazy creations of the same child map converge to the same container.

#### Deletion: native map key removal

Deleting a bookmark = removing its key from the root `bookmarks` map. This is a **proper CRDT operation**:
- The deletion propagates to all peers with causal ordering.
- Concurrent edits to the same bookmark (update title while another peer deletes it) resolve deterministically — either the update is visible and the delete loses, or vice versa, depending on lamport ordering.
- No soft-delete `deleted` flag needed — the presence or absence of the map key is the ground truth.

Tradeoff: you can't "undelete" or browse deleted entries. For a personal bookmark manager this is acceptable; if needed later, we can add a trash-collection layer on top (move deleted keys to a separate `trash` map instead of truly removing them).

---

## 3. CRDT Operations (LoroDoc Wrapper)

The `meshlet_core::doc` module exposes a `LoroStore` that wraps a `LoroDoc`:

```rust
pub struct LoroStore {
    doc: LoroDoc,
}

impl LoroStore {
    pub fn new() -> Self;
    pub fn from_snapshot(data: &[u8]) -> Result<Self>;
    pub fn export_snapshot(&self) -> Result<Vec<u8>>;
    pub fn export_updates_since(&self, vv: &VersionVector) -> Result<Vec<u8>>;
    pub fn import(&self, data: &[u8]) -> Result<ImportStatus>;
    pub fn oplog_vv(&self) -> VersionVector;
    pub fn state_vv(&self) -> VersionVector;

    // CRUD operations — each commits to loro
    pub fn add_bookmark(&self, b: &Bookmark) -> Result<()>;
    pub fn update_bookmark(&self, id: &BookmarkId, patch: &BookmarkPatch) -> Result<()>;
    pub fn delete_bookmark(&self, id: &BookmarkId) -> Result<()>;  // removes key from root map
    pub fn add_tags(&self, id: &BookmarkId, tags: &[String]) -> Result<()>;
    pub fn remove_tags(&self, id: &BookmarkId, tags: &[String]) -> Result<()>;
    pub fn get_bookmark(&self, id: &BookmarkId) -> Option<Bookmark>;
    pub fn list_bookmarks(&self) -> Vec<(BookmarkId, Bookmark)>;
}
```

Each CRUD operation works through the `LoroMap`:
- **add**: creates a new child map via `ensure_mergeable_map`, sets fields via `map.insert(...)`
- **update**: uses `BookmarkPatch` to set only changed fields on the existing child map
- **delete**: removes the ULID key from the root `bookmarks` map (native CRDT deletion)
- **tags**: add = `tags_map.insert(tag, true)`; remove = `tags_map.delete(tag)`

Loro's **auto-commit** mode is enabled. Each high-level operation calls `doc.commit()` when done.

### Applying Diffs (for incremental mirror updates)

After `import` (receiving remote changes), `subscribe_root` fires with structured `DiffEvent`s. The `mirror.rs` module handles these incrementally — no full index rebuild needed. See §4.2.

---

## 4. Local Database (SQLite)

SQLite serves two roles:

### 4.1 CRDT Persistence (source of truth)

```sql
CREATE TABLE loro_doc (
    id        INTEGER PRIMARY KEY,
    snapshot  BLOB,                     -- doc.export(ExportMode::Snapshot)
    vv        BLOB                      -- last saved VersionVector (encoded)
);
```

Persistence strategy (rewrite-on-commit, start simple):
1. **Startup**: load `snapshot` → `LoroDoc::from_snapshot()`. Mirror tables are rebuilt from the doc on first run if empty.
2. **On commit** (via `subscribe_local_update`): rewrite the snapshot BLOB. Bookmarks change rarely, so rewrite is cheap.
3. The `vv` column stores the `oplog_vv()` at save time for sync state tracking.
4. `subscribe_root` keeps mirror tables in sync incrementally — no need to rebuild on every change.

### 4.2 Materialized Search Index (for fast queries)

```sql
CREATE TABLE bookmarks (
    id          TEXT PRIMARY KEY,        -- ULID string
    url         TEXT NOT NULL,
    title       TEXT NOT NULL DEFAULT '',
    desc        TEXT NOT NULL DEFAULT '',
    immutable_title INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER,                 -- Unix timestamp (i64), derived from ChangeMeta
    updated_at  INTEGER                  -- Unix timestamp (i64), derived from ChangeMeta
);
CREATE TABLE bookmark_tags (
    bookmark_id TEXT NOT NULL REFERENCES bookmarks(id) ON DELETE CASCADE,
    tag         TEXT NOT NULL,
    PRIMARY KEY (bookmark_id, tag)
);
CREATE INDEX idx_bookmark_tags_tag ON bookmark_tags(tag);
CREATE INDEX idx_bookmarks_url ON bookmarks(url);
-- FTS5 virtual table on (url, title, desc) deferred to M7
```

**Mirror sync via `subscribe_root`**:

The `meshlet_core::mirror` module subscribes to `doc.subscribe_root()` and receives structured `DiffEvent`s after every commit (local or imported). It applies these diffs incrementally:
- New bookmark map → `INSERT INTO bookmarks` + `INSERT INTO bookmark_tags`
- Updated fields → `UPDATE bookmarks` / `REPLACE INTO bookmark_tags`
- Bookmark map removed from root → `DELETE FROM bookmarks` (cascades to `bookmark_tags`)

This is the **correct pattern** and loro is designed for it.

**Custom SQLite functions** (registered at connection open, like buku):
- `REGEXP` — for regex search (rusqlite allows registering custom functions)
- `NETLOC` — extracts netloc from URL for domain-based queries

---

## 5. Sync Protocol

### 5.1 Architecture

```
┌──────────────┐         ┌──────────────┐         ┌──────────────┐
│  Client A    │ ◄─────► │   Server     │ ◄─────► │  Client B    │
│  (LoroDoc)   │   sync  │  (LoroDoc)   │   sync  │  (LoroDoc)   │
│  + SQLite    │         │  + storage   │         │  + SQLite    │
└──────────────┘         └──────────────┘         └──────────────┘
```

- The server is **another loro peer** — it maintains a full LoroDoc copy
- The server is a **relay**, not an authority — clients are the source of truth
- The server doesn't need SQLite (stores serialized LoroDoc snapshots on disk)
- Clients can function fully offline; sync is optional

### 5.2 Sync Flow (HTTP request-response, single round-trip)

Single endpoint: `POST /sync` with header `Authorization: Bearer <token>`.

**Request** (CBOR or JSON):
```json
{
  "client_vv": "<bytes>",     // client.oplog_vv(), encoded
  "updates":   "<bytes>"      // client.export(ExportMode::updates(&server_vv))
}
```

**Response**:
```json
{
  "server_vv": "<bytes>",     // server.oplog_vv(), encoded
  "updates":   "<bytes>"      // server.export(ExportMode::updates(&client_vv))
}
```

**Flow**:
1. Client sends its VV and any updates the server is missing
2. Server imports client updates, then exports updates the client is missing
3. Client imports server's response
4. Client persists the new snapshot and `last_known_server_vv`
5. `subscribe_root` fires, mirror updates SQLite incrementally

**Key details:**
- Because each side sends only what the other is missing, **one round-trip per sync** is sufficient for the common case.
- The `updates` field may be empty if neither side has new changes.
- All merging is conflict-free thanks to CRDTs.
- The client is fully functional without the server. Changes accumulate as local CRDT ops and merge cleanly on next successful sync.

### 5.3 URL Deduplication (reconciliation on import)

When two devices independently bookmark the same URL while offline, they produce **two different ULIDs**. After import merges the CRDT state, we run a deterministic reconciliation pass:

1. Query the mirror for all `(id, url)` pairs where the same normalized URL appears under multiple ULIDs.
2. For each duplicate set, pick the **winner** as the entry with the lowest `created_at` timestamp (tie-break: lexicographically lower ULID).
3. Merge into the winner: copy non-empty fields from losers, union all tags, keep the winner's `created_at`, use the maximum `updated_at`.
4. Delete the loser entries from the CRDT (remove their keys from the root map).

This reconciliation is expressed as **CRDT ops** (not SQL-only), so every peer that imports the updates performs the same deterministic merge. The import → reconcile → mirror-update chain ensures both the CRDT and the SQL projection converge cleanly.

**URL normalization** for dedup purposes: lowercase scheme + host, strip trailing slash, keep path/query. Exact rules TBD during implementation (e.g., should we strip `www.` prefix? UTM params?).

### 5.4 Peer Identity

- Each client generates a random `PeerID` on first run, persisted in SQLite
- The server has a fixed `PeerID`
- `PeerID` must NOT be reused across devices/sessions (loro requirement)
- If a user copies their SQLite DB to a new machine, they must generate a new `PeerID`

### 5.5 Server Storage

The server's LoroDoc is the **sole persistent state**. It is saved as a binary snapshot file on disk:
- On shutdown / periodically: write `doc.export(ExportMode::Snapshot)` to `meshlet-server.state`
- On startup: `LoroDoc::from_snapshot(&bytes)` → ready

No SQLite on the server. The server is a pure CRDT relay peer.

---

## 6. Web Fetching (Auto-fetch Title, Description, Tags)

Porting buku's `fetch_data()` from Python to Rust:

### 6.1 HTTP Client
- **reqwest** with `rustls-tls` for HTTPS
- Follow redirects (reqwest does this by default)
- User-Agent header (mimic a browser)
- Configurable timeouts

### 6.2 HTML Parsing
- **scraper** crate (CSS selector-based, like BeautifulSoup)
- Extract:
  - **Title**: `<title>` tag text content
  - **Description**: `<meta name="description" content="...">`
  - **Tags/Keywords**: `<meta name="keywords" content="...">`

### 6.3 Edge Cases (ported from buku)
- Non-HTML URLs (PDF, images): skip fetching, just store the URL
- HTTP errors (403, 404, etc.): store with status info
- Redirects: store the final URL after redirects
- Malformed URLs: reject early
- Encoding detection: use `charset` from Content-Type header or `<meta charset>`

### 6.4 API

```rust
pub struct FetchResult {
    pub final_url: String,
    pub title: Option<String>,
    pub desc: Option<String>,
    pub tags: Vec<String>,
    pub status: u16,
    pub is_mime: bool,    // true if non-HTML (HEAD request only)
    pub bad: bool,        // true if URL is malformed
}

pub async fn fetch_bookmark_data(url: &str) -> FetchResult;
```

---

## 7. CLI Design

### 7.1 Commands

```
meshlet add <url> [options]
    --title <title>       Set title manually (skip auto-fetch)
    --tag <tags>          Comma-separated tags
    --desc <desc>         Description
    --no-fetch            Don't fetch from web
    --immutable           Don't auto-update title on future fetches

meshlet search <keywords...> [options]
    --deep                Search substrings, not just words
    --regex               Treat keywords as regex
    --all                 Match ALL keywords (default: match ANY)
    --tag <tags>          Filter by tags
    --json                Output as JSON

meshlet edit <index> [options]
    --url <url>           Change URL
    --title <title>       Change title
    --tag <tags>          Replace tags
    --tag-add <tags>      Append tags
    --tag-delete <tags>   Remove tags
    --desc <desc>         Change description

meshlet delete <index...>
    --range <low> <high>  Delete range of indices

meshlet list [options]
    --tag <tags>          Filter by tag
    --json                Output as JSON

meshlet open <index>      Open bookmark in browser
meshlet import <file>     Import from HTML/bookmark files
meshlet export <file>     Export to HTML/Markdown/db format

meshlet sync              Trigger manual sync with server
    --status              Show sync status (last sync time, pending changes)

meshlet config            Show/set configuration
    --server <url>        Set sync server URL
    --token <token>       Set auth token
```

### 7.2 Output Format (matching buku)

```
$ meshlet list
  1. Example Page [tag1,tag2]
     > https://example.com
     + A description of the page
     # tag1, tag2

$ meshlet search rust crdt
  3. Loro CRDT [rust,crdt]
     > https://loro.dev
     + High-performance CRDT framework
     # rust, crdt
```

### 7.3 Configuration

Configuration file: `~/.config/meshlet/config.toml`

```toml
[server]
url = "https://sync.example.com"
token = "my-secret-token"

[display]
color = true
show_url = true
show_desc = true
show_tags = true
```

Data directory: `~/.local/share/meshlet/`
- `bookmarks.db` — SQLite database (CRDT persistence + search index)

---

## 8. Key Dependencies

| Crate | Purpose |
|-------|---------|
| `loro = "1"` | CRDT document |
| `rusqlite = { version = "0.32", features = ["bundled"] }` | SQLite (snapshot blob + mirror) |
| `reqwest = { version = "0.12", features = ["blocking"] }` | Title/desc fetching |
| `scraper = "0.20"` | HTML parsing (html5ever, equivalent to buku's html5lib) |
| `clap = { version = "4", features = ["derive"] }` | CLI argument parsing |
| `ulid = "1"` | Bookmark IDs (time-sortable, coordination-free) |
| `tokio` | Async runtime (fetcher + server) |
| `axum` | HTTP server (M6) |
| `anyhow` / `thiserror` | Error handling |
| `tempfile` | Editor integration |
| `tracing` + `tracing-subscriber` | Logging |
| `ciborium` | Sync payload encoding (M6, via `meshlet-proto`) |
| `url` | URL normalization/parsing |
| `dirs` | Config/data directories |
| `colored` | Terminal colors |
| `webbrowser` | Open URLs in browser |

| `uuid` | NOT needed — ULID replaces UUID |
| `chrono` | NOT needed — loro's native timestamps replace chrono |

---

## 9. Milestones

Each milestone must pass: `cargo check --workspace`, `cargo test -p <crate>`, `cargo clippy --workspace -- -D warnings` (added from M2 onward).

### M1 — Workspace + core data model
**Goal**: Workspace scaffold; in-memory CRDT round-trip.

- [ ] Create Cargo workspace with 4 crates (core, proto, cli, server)
- [ ] `model.rs`: `Bookmark` struct, `BookmarkId` (ULID)
- [ ] `doc.rs`: `LoroStore` — `new`, `from_snapshot`, `export_snapshot`
- [ ] CRUD via `ensure_mergeable_map`: `add`, `get`, `update`, `delete` on the bookmarks map
- [ ] Tag operations: `add_tags`, `remove_tags` via `LoroMap{tag → true}`
- [ ] `set_record_timestamp(true)` enabled from day 1
- [ ] `error.rs`: `thiserror` error types
- [ ] `meshlet-proto/messages.rs`: `SyncRequest`, `SyncResponse` (serde), VV encoding
- [ ] Unit test: create bookmarks with tags, serialize snapshot, deserialize, verify round-trip
- [ ] Unit test: two `LoroStore` instances (simulating two peers) with concurrent adds — verify convergence

### M2 — SQLite mirror + persistence
**Goal**: State survives process restart; CRDT changes reflected in queryable SQL.

- [ ] `store.rs`: open/create SQLite DB with `loro_doc` + mirror tables (`bookmarks`, `bookmark_tags`)
- [ ] Schema migrations (simple version integer)
- [ ] `mirror.rs`: `subscribe_root` → incremental SQL upsert/delete (including native map deletion → `DELETE` cascade)
- [ ] Snapshot persistence: rewrite-on-commit via `subscribe_local_update`
- [ ] `search.rs`: keyword/tag query over the mirror tables
- [ ] Custom SQLite functions: `REGEXP`, `NETLOC`
- [ ] Unit test: add bookmark → query mirror → restart → load → query mirror

### M3 — Title fetcher
**Goal**: `add` auto-fetches title, description, and keywords from the web.

- [ ] `fetch.rs`: `reqwest` + `scraper` extract `<title>`, `<meta description>`, `<meta keywords>`
- [ ] Handle non-HTML URLs, HTTP errors, redirects, encoding detection
- [ ] `--no-fetch` / `--immutable` flags honored
- [ ] Unit test: offline HTML fixtures (no network dependency in tests)

### M4 — CLI MVP
**Goal**: Feature-parity with the most common buku commands.

- [ ] `args.rs`: clap derive with subcommands: `add`, `list`, `search`, `delete`, `edit`, `tag`
- [ ] `add`: URL + optional title/tags/desc, auto-fetch by default
- [ ] `list`: all bookmarks or filtered by tag; display sequential index numbers
- [ ] `search`: keywords, `--deep`, `--regex`, `--tag` filter, `--all` mode
- [ ] `delete`: by index (removes key from root `bookmarks` map — native CRDT deletion)
- [ ] `edit` / `tag`: update fields, add/remove tags
- [ ] Formatted terminal output (colors, fields like buku)

### M5 — Editor + import/export
**Goal**: `$EDITOR` flow; HTML + Markdown import/export.

- [ ] `editor.rs`: tempfile + `$EDITOR` with 4-line template (url/title/tags/comments)
- [ ] `import_export.rs`: import from Netscape/Firefox `bookmarks.html`
- [ ] `import_export.rs`: export to HTML, Markdown
- [ ] `open` command: launch URL in default browser
- [ ] Configuration file: `~/.config/meshlet/config.toml`

### M6 — Server crate + sync
**Goal**: Multi-device sync working end-to-end.

- [ ] `meshlet-server/transport.rs`: axum HTTP server, `POST /sync`, bearer token auth
- [ ] `meshlet-server/doc.rs`: server LoroDoc + snapshot persistence to file
- [ ] Client `sync` subcommand: connect to server, exchange updates using `meshlet-proto` types
- [ ] `last_known_server_vv` persisted in SQLite for efficient subsequent syncs
- [ ] `reconcile.rs`: URL deduplication pass on import (deterministic, CRDT-expressed)
- [ ] Integration test: two clients with offline writes → sync through server → both converge
- [ ] Integration test: two clients independently bookmark same URL offline → sync → dedup merges entries

### M7 (later, not in initial scope)
- [ ] Encryption at rest
- [ ] FTS5 full-text search on mirror tables
- [ ] Interactive REPL (`meshlet` without subcommand, like buku)
- [ ] Org/RSS/Atom export formats
- [ ] Browser auto-import (Chrome/Firefox bookmarks files)
- [ ] Per-bookmark notes (additional `LoroText` field)
- [ ] Tag autocomplete

---

## 10. Locked-in Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| CRDT library | **loro** | Modern Rust CRDT with rich data types (Map, List, Text). No built-in networking — we build our own sync. |
| One LoroDoc vs per-bookmark docs | **One LoroDoc** (Map) | Simpler sync (one VV), Map CRDT handles concurrent adds naturally |
| IDs | **ULID** | Time-sortable, no coordination, shorter than UUID, lexicographic sort = chronological sort |
| Tags representation | **`LoroMap{tag → true}`** | Map keys enforce uniqueness — no duplicate tags on concurrent adds. Remove = delete key. True CRDT set semantics. |
| Map creation | **`ensure_mergeable_map`** | Deterministic container IDs so two peers creating same ULID converge, not conflict |
| Timestamps | **`set_record_timestamp(true)`** | Loro records Unix timestamps automatically per ChangeMeta — no manual timestamp fields needed in the map |
| Title / Description text | **Plain `LoroValue::String`** (LWW) | NOT `LoroText`. Per-character collaborative editing is overkill for bookmark metadata and costs more space |
| Deletion | **Native map key removal** | Remove the ULID key from the root `bookmarks` map — proper CRDT operation with causal propagation. No soft-delete flag. |
| SQLite mirror sync | **`subscribe_root`** (incremental) | Loro's designed pattern for keeping external indexes in sync; no full rebuilds |
| Snapshot durability | **Rewrite-on-commit** snapshot BLOB | Bookmarks change rarely; rewrite is cheap. Append-only updates log deferred unless latency becomes a problem. |
| Sync transport | **HTTP `POST /sync`** (request-response) | Simpler server; client initiates every sync. One round-trip per sync is sufficient. |
| Server role | **Relay** (CRDT peer) | No server-side business logic; clients are authoritative; simpler architecture |
| Server auth | **Shared-secret bearer token** over TLS | Simple, adequate for personal sync. Designed in M6, not earlier. |
| URL deduplication | **Deterministic CRDT reconciliation on import** | Two devices bookmarking same URL offline → merge into earliest entry, expressed as CRDT ops so all peers converge |
| v1 scope | **Offline-only CLI first** (M1–M5), server + sync last (M6) | Validates core data model and CRUD UX before locking the wire protocol |
| Async runtime | **tokio** | Industry standard; needed for reqwest and axum |

---

## 11. Open Questions / Future Work

1. **Garbage collection**: LoroDoc grows monotonically. For very long-lived databases, `compact_change_store()` can free memory, and periodic fresh-snapshot resets can prune history.

2. **End-to-end encryption**: Currently, the server sees all bookmarks in plaintext. For privacy, we could encrypt the LoroDoc bytes before sending to the server (the server would just relay opaque blobs).

3. **Conflict resolution UX**: While CRDTs resolve conflicts automatically, showing a "sync summary" to the user could be valuable (e.g., "3 bookmarks synced, 2 updated by another device").

4. **Browser extensions**: A companion browser extension for one-click bookmarking.

5. **Tag autocomplete**: Fuzzy-find existing tags when adding/editing.

6. **Full-text search in page content**: Download and index the full text of bookmarked pages for deep search (like a personal search engine). Far beyond buku's scope.

7. **URL normalization depth**: How aggressively should we normalize for dedup? Strip `www.`? UTM params? Tracking query strings? Plan: start conservative (lowercase scheme+host, strip trailing slash) and add rules based on real-world use.
