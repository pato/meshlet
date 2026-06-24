# meshlet

> [!WARNING]  
> This is mostly vibe coded (there was guidance and architecture, but not
> proper AI engineering) and intended to be used solely for my specific use
> case, so proceed at your own risk if you wish to use/run this.

A bookmark manager for the command line. Stores bookmarks locally in SQLite, auto-fetches titles and descriptions from the web, and syncs between machines through a self-hosted server using CRDTs (so offline edits never conflict).

Inspired by [buku](https://github.com/jarun/buku), with first-class multi-device sync.


## Build

Requires Rust 1.85+.

```sh
cargo build --release
# binaries land in:
#   target/release/meshlet-cli
#   target/release/meshlet-server
```

## Quick start

```sh
meshlet add https://loro.dev --tag rust,crdt
meshlet list
meshlet search rust
meshlet open 1          # opens the first bookmark in your browser
meshlet edit 1 --title "New title"
meshlet delete 1
```

Run `meshlet add` without a URL to drop into `$EDITOR` with a 4-line template (url / title / tags / description).

### Adding bookmarks

```
meshlet add <url> [--title T] [--tag t1,t2] [--desc D] [--no-fetch] [--immutable]
```

By default meshlet fetches the page and fills in title, description, and keywords from `<meta>` tags. Pass `--no-fetch` to skip that, or `--title`/`--desc` to override. `--immutable` freezes the title so future fetches won't overwrite it.

### Listing and searching

```
meshlet list [--tag rust] [--json]
meshlet search <keywords...> [--deep] [--regex] [--all] [--tag t] [--json]
```

- `--deep` matches substrings instead of tokens
- `--regex` treats the keywords as a regex (joined with `|`)
- `--all` requires all keywords to match (default: any)
- `--tag` filters by tag in addition to the text search

Output is the classic buku-style numbered list:

```
    1. Loro CRDT [crdt,rust]
       > https://loro.dev
       + A high-performance CRDT framework
       # crdt, rust
```

### Editing, tagging, deleting

```
meshlet edit <index> [--url U] [--title T] [--desc D] [--tag T] [--tag-add T] [--tag-delete T] [--immutable on|off]
meshlet tag <index> <tags...> [--delete]
meshlet delete <index...> [--range 1 5]
```

Indices are 1-based and match what `list`/`search` print.

### Import / export

```sh
meshlet import bookmarks.html        # Netscape/Firefox format
meshlet export out.md                # Markdown
meshlet export out.html --format html
```

Imported entries get an `imported` tag added automatically.

## Syncing between machines

Meshlet syncs through a small relay server that you run somewhere both machines can reach. The server is a dumb CRDT peer — it just stores and forwards updates. All your data also lives locally, so the server being down never breaks the CLI.

### 1. Run the server

```sh
meshlet-server --bind 0.0.0.0:3000 --token "a-long-random-secret"
```

State is kept in a single file under `--data-dir` (default: platform data dir / `meshlet-server/meshlet-server.state`). There's no database to operate.

Put it behind TLS (a reverse proxy like Caddy or nginx) — the token is sent in cleartext otherwise.

### 2. Point your clients at it

Either pass flags each time:

```sh
meshlet sync --server https://sync.example.com --token "a-long-random-secret"
```

Or set it once and forget:

```sh
meshlet config --server https://sync.example.com --token "a-long-random-secret"
meshlet sync                # uses the saved config
meshlet sync --status       # shows whether you've synced before
```

After the first sync, meshlet only exchanges the changes each side is missing, so subsequent syncs are usually a single quick round-trip.

### Reconciling duplicates

If two machines bookmark the same URL while offline, meshlet notices after sync and merges them into a single entry — keeping the earliest creation time and unioning the tags. URL comparison strips `www.`, trailing slashes, fragments, and common tracking params (`utm_*`, `fbclid`, `gclid`, …).

## Configuration

`~/.config/meshlet/config.toml` (or your platform's config dir):

```toml
data_dir = "/path/to/bookmarks"   # optional, defaults to ~/.local/share/meshlet

[server]
url = "https://sync.example.com"
token = "a-long-random-secret"

[display]
color = true         # ANSI colors in output
show_url = true
show_desc = true
show_tags = true
```

Data lives in `~/.local/share/meshlet/bookmarks.db` (or platform equivalent). To use a different location, either set `data_dir` in the config file (`meshlet config --data-dir /path`) or export the `MESHLET_DATA_DIR` environment variable. The env var takes precedence over the config file, which takes precedence over the default.
