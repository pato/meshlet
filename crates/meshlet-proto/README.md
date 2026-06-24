# meshlet-proto

Sync wire types for [meshlet](https://github.com/pato/meshlet) — `SyncRequest` and `SyncResponse` with base64-encoded Loro CRDT updates. Used by both the CLI client and the relay server.

This crate is only useful if you're building a custom sync client or server that interoperates with meshlet.

## Related crates

- [`meshlet-cli`](https://crates.io/crates/meshlet-cli) — the user-facing CLI binary
- [`meshlet-server`](https://crates.io/crates/meshlet-server) — the sync relay server
- [`meshlet-core`](https://crates.io/crates/meshlet-core) — the underlying library
