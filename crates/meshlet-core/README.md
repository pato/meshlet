# meshlet-core

Core library for [meshlet](https://github.com/pato/meshlet) — the bookmark manager's data layer. Provides:

- **CRDT storage** via [Loro](https://loro.dev) — conflict-free bookmark CRUD with deterministic convergence
- **SQLite mirror** — queryable projection of the CRDT state for fast keyword/tag/regex search
- **Web fetcher** — extracts title, description, and keywords from HTML pages
- **Reconciliation** — deduplicates bookmarks by URL after multi-device sync

You probably want [`meshlet-cli`](https://crates.io/crates/meshlet-cli) (the binary) rather than this library directly.

## Related crates

- [`meshlet-cli`](https://crates.io/crates/meshlet-cli) — the user-facing CLI binary
- [`meshlet-server`](https://crates.io/crates/meshlet-server) — the sync relay server
- [`meshlet-proto`](https://crates.io/crates/meshlet-proto) — sync wire types
