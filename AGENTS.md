# AGENTS.md

Context for AI agents working in this repo. Read this before making changes.

## Read PLAN.md first

`PLAN.md` is the authoritative design document: architecture, data model, CRDT schema, sync protocol, milestone definitions. Do not duplicate that content here — refer to it. This file covers practical working context that the plan doesn't spell out.

## Project

meshlet is a CLI bookmark manager (buku-like) with multi-device CRDT sync via a self-hosted relay server. Rust workspace, edition 2021, 4 crates.

## Build / test / lint

```sh
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings   # strict, must be clean
cargo build --workspace --release
```

Clippy with `-D warnings` is a hard gate (PLAN §9). Don't commit if it isn't clean. After `cargo clean` the workspace builds in ~15s.

Binaries: `meshlet-cli` (the user-facing CLI), `meshlet-server` (the sync relay).

## Workspace layout

```
crates/
├── meshlet-core/     # LoroDoc CRDT + SQLite mirror + fetch + reconcile (lib)
├── meshlet-proto/    # Sync wire types (SyncRequest/SyncResponse) — used by cli and server
├── meshlet-cli/      # The binary (clap + editor + import/export)
└── meshlet-server/   # Sync relay binary + lib (lib/bin split so tests can reach the router)
```

Dep graph: `cli → core, proto`. `server → core, proto`. `proto → loro, serde`. `core` depends on nothing in this workspace.

## Critical conventions

**Errors.** `meshlet-core` uses `thiserror` with `MeshletError` and a `Result<T>` alias in `src/error.rs`. Binaries (`cli`, `server`) use `anyhow::Result`. Don't introduce `anyhow` in core.

**CRDT ops (doc.rs).** Always use `ensure_mergeable_map` when creating a child `LoroMap` — never `get_map` for creation. This is what lets two peers independently creating the same ULID converge instead of conflicting. Call `doc.commit()` at the end of each high-level operation. Tags are `LoroMap{tag → true}` (NOT a list — keys enforce uniqueness).

**Mirror sync.** The SQLite mirror is updated manually in `MeshletDb` (lib.rs), NOT via `subscribe_root` as PLAN §4.2 describes. Each CRUD method calls `db.upsert_bookmark(...)` / `db.delete_bookmark_mirror(...)` then `save_snapshot()`. `import()` and `sync_import()` do a full `rebuild_mirror()` after importing remote changes. See "Known deviations" below before changing this.

**CLI index convention.** All commands use 1-based top-down indexing: `bookmarks[index - 1]` where index 1 is the first row printed by `list`/`search`. If you add a new command that takes an index, follow the same pattern.

**Adding a CLI command.** Edit `args.rs` (add a `Commands` variant) and `main.rs` (add a `match` arm + a `cmd_*` fn). The `cmd_*` functions open `MeshletDb::open(&db_path()?)?` locally — they don't share a connection across commands.

**Config.** `~/.config/meshlet/config.toml` is read by `load_config()` in `main.rs`. `Config`, `ServerConfig`, `DisplayConfig` derive `Serialize + Deserialize` so `meshlet config --server ... --token ...` can write the file back. The data directory is resolved in this order: `MESHLET_DATA_DIR` env var → `data_dir` field in config.toml → platform default (`~/.local/share/meshlet/`). There is no `--data-dir` CLI flag.

## Known deviations from PLAN.md (conscious decisions, not bugs)

Do not "fix" these without explicit instruction — they were chosen during implementation:

- **No `mirror.rs` / no `subscribe_root`.** Mirror updates are manual writes in `MeshletDb`. PLAN §4.2 and M2 describe an incremental `subscribe_root`-based approach; it was simplified to direct writes + full rebuild on import.
- **Timestamps are stored as explicit LWW fields** (`FIELD_CREATED_AT`, `FIELD_UPDATED_AT` in doc.rs), not derived from `ChangeMeta`. `set_record_timestamp(true)` is still called, but the values actually read come from the map fields. PLAN §2.2 claims timestamps are derived from the op log.
- **Sync encoding is JSON + base64**, not CBOR. PLAN §8 lists `ciborium`; the implementation uses `serde_json` + `base64` in `meshlet-proto`.
- **`meshlet-server` is a lib + bin split** (not pure binary as PLAN §1 implies) so integration tests in `tests/sync_e2e.rs` can call the axum router via `tower::ServiceExt::oneshot`.

## Testing patterns

- **Unit tests** are inline (`#[cfg(test)] mod tests { ... }`) at the bottom of each module.
- **Integration tests** live in `crates/<name>/tests/`:
  - `meshlet-core/tests/sync_integration.rs` — pure CRDT-level sync loop (two `MeshletDb` swapping updates directly, no HTTP). Tests convergence, dedup, and mirror-after-restart.
  - `meshlet-server/tests/sync_e2e.rs` — HTTP-level via `tower::ServiceExt::oneshot` against the real axum router. Tests the wire protocol, serde, and bearer auth. No socket binding, no subprocess.
- When adding sync-related tests, pick the right layer: CRDT math → core tests; HTTP/auth/serialization → server tests.
- The `tower` dev-dep in `meshlet-server` has the `util` feature for `oneshot`.

## Things that look wrong but aren't

- `meshlet-cli/Cargo.toml` lists `reqwest` with `default-features = false` and only `["blocking", "json"]`, yet HTTPS sync works. This is because `meshlet-core`'s reqwest enables `rustls-tls`, and Cargo's feature unification brings it into the CLI build. Don't add `rustls-tls` to the CLI thinking it's missing.
- `meshlet-core/src/lib.rs` does `pub use loro;` and `pub use rusqlite;` so binaries and tests can reference `meshlet_core::loro::VersionVector` without adding those crates as direct deps.
- `ServerDoc` sets `peer_id(0)` (doc.rs). That's intentional for the relay — it doesn't contribute edits, only forwards.
- `reconcile` reads the winner snapshot once and doesn't re-read between losers. For the common 2-entry duplicate case this is correct; for 3+ duplicates of the same URL only the first loser's title/desc gets merged. Acceptable for v1.

## Commit message style

Look at `git log --oneline`. Pattern is `<scope>: <summary` on the first line, then categorized bullet blocks (Critical fixes / Features / Tests / …). Scopes have been milestone-based (`M4:`, `M6:`) or `v1 polish round N:` for follow-ups. Lowercase, imperative, no emoji.

## Don't

- Don't add comments to code unless asked. Code is intentionally terse; rationale lives in PLAN.md or commit messages.
- Don't commit unless explicitly asked.
- Don't introduce new deps without checking an existing equivalent is already used (see PLAN §8 for the curated list).
- Don't enable `default-features` on `reqwest` in `meshlet-cli` — see "Things that look wrong" above.
