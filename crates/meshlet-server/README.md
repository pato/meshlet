# meshlet-server

The sync relay server for [meshlet](https://github.com/pato/meshlet). It's a CRDT peer that stores and forwards bookmark updates between your devices — it has no business logic and sees all data in plaintext (use TLS).

## Install

```sh
cargo install meshlet-server
```

## Run

```sh
meshlet-server --bind 0.0.0.0:3000 --token "your-secret"
```

State is kept in a single snapshot file under `--data-dir`. No database to operate.

Put it behind TLS (Caddy, nginx, etc.) — the bearer token is sent in cleartext otherwise.

## Pointing clients at it

```sh
meshlet-cli config --server https://sync.example.com --token "your-secret"
meshlet-cli sync
```

## Related crates

- [`meshlet-cli`](https://crates.io/crates/meshlet-cli) — the user-facing CLI binary
- [`meshlet-core`](https://crates.io/crates/meshlet-core) — the underlying library
- [`meshlet-proto`](https://crates.io/crates/meshlet-proto) — sync wire types
