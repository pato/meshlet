# meshlet-cli

A command-line bookmark manager with multi-device CRDT sync. Stores bookmarks locally, auto-fetches titles and descriptions from the web, and syncs between machines through a self-hosted relay server.

This is the user-facing binary — it's what you install to use meshlet.

## Install

```sh
cargo install meshlet-cli
```

## Quick start

```sh
meshlet add https://loro.dev --tag rust,crdt
meshlet list
meshlet search rust
meshlet open 1
```

Run `meshlet --help` for the full command list.

## Related crates

- [`meshlet-server`](https://crates.io/crates/meshlet-server) — the sync relay server binary
- [`meshlet-core`](https://crates.io/crates/meshlet-core) — the underlying library (CRDT storage, fetcher, search)
- [`meshlet-proto`](https://crates.io/crates/meshlet-proto) — sync wire types

Full documentation: [github.com/pato/meshlet](https://github.com/pato/meshlet)
