mod args;
mod editor;
mod import_export;

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use meshlet_core::model::{Bookmark, BookmarkId, BookmarkPatch};
use meshlet_core::MeshletDb;
use serde::{Deserialize, Serialize};

use args::{Cli, Commands};

#[derive(Debug, Default, Serialize, Deserialize)]
struct Config {
    #[serde(default)]
    data_dir: Option<String>,
    #[serde(default)]
    server: ServerConfig,
    #[serde(default)]
    display: DisplayConfig,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ServerConfig {
    url: Option<String>,
    token: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct DisplayConfig {
    color: Option<bool>,
    show_url: Option<bool>,
    show_desc: Option<bool>,
    show_tags: Option<bool>,
}

fn data_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("MESHLET_DATA_DIR") {
        if !dir.is_empty() {
            let path = PathBuf::from(dir);
            std::fs::create_dir_all(&path).context("could not create data directory")?;
            return Ok(path);
        }
    }

    let config = load_config();
    if let Some(ref dir) = config.data_dir {
        if !dir.is_empty() {
            let path = PathBuf::from(dir);
            std::fs::create_dir_all(&path).context("could not create data directory")?;
            return Ok(path);
        }
    }

    let dir = dirs::data_dir()
        .context("could not find data directory")?
        .join("meshlet");
    std::fs::create_dir_all(&dir).context("could not create data directory")?;
    Ok(dir)
}

fn config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("could not find config directory")?
        .join("meshlet");
    std::fs::create_dir_all(&dir).context("could not create config directory")?;
    Ok(dir)
}

fn load_config() -> Config {
    let path = config_dir().ok().map(|d| d.join("config.toml"));
    match path.and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(content) => toml::from_str(&content).unwrap_or_default(),
        None => Config::default(),
    }
}

fn db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("bookmarks.db"))
}

fn main() -> Result<()> {
    let config = load_config();
    if !config.display.color.unwrap_or(true) {
        colored::control::set_override(false);
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Add {
            url,
            title,
            tag,
            desc,
            no_fetch,
            immutable,
        } => cmd_add(url.as_deref(), title.as_deref(), tag.as_deref(), desc.as_deref(), no_fetch, immutable),
        Commands::List { tag, json } => cmd_list(tag.as_deref(), json),
        Commands::Search {
            keywords,
            deep,
            regex,
            all,
            tag,
            json,
        } => cmd_search(&keywords, deep, regex, all, tag.as_deref(), json),
        Commands::Delete { indices, range } => cmd_delete(&indices, range.as_deref()),
        Commands::Edit {
            index,
            url,
            title,
            tag,
            tag_add,
            tag_delete,
            desc,
            immutable,
        } => cmd_edit(
            index,
            url.as_deref(),
            title.as_deref(),
            tag.as_deref(),
            tag_add.as_deref(),
            tag_delete.as_deref(),
            desc.as_deref(),
            immutable,
        ),
        Commands::Tag { index, tags, delete } => cmd_tag(index, &tags, delete),
        Commands::Open { index } => cmd_open(index),
        Commands::Import { file } => cmd_import(&file),
        Commands::Export { file, format } => cmd_export(&file, &format),
        Commands::Sync { server, token, status } => {
            if status {
                cmd_sync_status()
            } else {
                cmd_sync(server.as_deref(), token.as_deref())
            }
        }
        Commands::Gc => cmd_gc(),
        Commands::Config { server, token, data_dir } => cmd_config(server.as_deref(), token.as_deref(), data_dir.as_deref()),
    }
}

fn cmd_add(
    url: Option<&str>,
    title: Option<&str>,
    tags: Option<&str>,
    desc: Option<&str>,
    no_fetch: bool,
    immutable: bool,
) -> Result<()> {
    let (url_str, edit_title, edit_tags, edit_desc) = if let Some(u) = url {
        (u.to_string(), title.map(String::from), tags.map(String::from), desc.map(String::from))
    } else {
        match editor::open_editor(None)? {
            Some(data) => (
                data.url,
                if data.title.is_empty() { None } else { Some(data.title) },
                if data.tags.is_empty() { None } else { Some(data.tags) },
                if data.desc.is_empty() { None } else { Some(data.desc) },
            ),
            None => {
                eprintln!("Editor closed without saving.");
                return Ok(());
            }
        }
    };

    let db = MeshletDb::open(&db_path()?)?;

    let tag_set = parse_tags(edit_tags.as_deref().unwrap_or(""));
    let mut flags: i64 = 0;
    if immutable {
        flags |= 0x01;
    }

    let (fetched_title, fetched_desc, fetched_tags) = if !no_fetch {
        let result = meshlet_core::fetch::fetch_bookmark_data(&url_str);
        if result.bad {
            eprintln!("{}: could not fetch URL (status {})", "warning".yellow(), result.status);
        }
        (
            edit_title.or(result.title),
            edit_desc.or(result.desc),
            result.tags,
        )
    } else {
        (edit_title, edit_desc, vec![])
    };

    let mut all_tags: BTreeSet<String> = tag_set;
    for t in fetched_tags {
        all_tags.insert(t);
    }

    let now = meshlet_core::model::now_ts();
    let bookmark = Bookmark {
        id: BookmarkId::new(),
        url: url_str,
        title: fetched_title.unwrap_or_default(),
        desc: fetched_desc.unwrap_or_default(),
        tags: all_tags,
        flags,
        created_at: now,
        updated_at: now,
    };

    db.add_bookmark(&bookmark)?;
    println!("Added bookmark: {}", bookmark.title.green());
    Ok(())
}

fn cmd_list(tag: Option<&str>, json: bool) -> Result<()> {
    let config = load_config();
    let db = MeshletDb::open(&db_path()?)?;
    let bookmarks = if let Some(tags) = tag {
        let tag_list: Vec<String> = parse_tags(tags).into_iter().collect();
        db.search_by_tags(&tag_list)?
    } else {
        db.list_from_mirror()?
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&bookmarks)?
        );
    } else {
        print!("{}", display_bookmarks(&bookmarks, &config.display));
    }
    Ok(())
}

fn cmd_search(
    keywords: &[String],
    deep: bool,
    regex: bool,
    all_match: bool,
    tag: Option<&str>,
    json: bool,
) -> Result<()> {
    let config = load_config();
    let db = MeshletDb::open(&db_path()?)?;

    let results = if regex && !keywords.is_empty() {
        let pattern = keywords.join("|");
        meshlet_core::search::search_regex(db.inner_connection(), &pattern, None)?
    } else {
        db.search_keywords(keywords, deep, all_match)?
    };

    let bookmarks = if let Some(tags) = tag {
        let tag_list: Vec<String> = parse_tags(tags).into_iter().collect();
        let tag_set: BTreeSet<String> = tag_list.into_iter().collect();
        results
            .into_iter()
            .filter(|b| tag_set.iter().any(|t| b.tags.contains(t)))
            .collect()
    } else {
        results
    };

    if bookmarks.is_empty() {
        println!("No results found.");
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&bookmarks)?);
    } else {
        print!("{}", display_bookmarks(&bookmarks, &config.display));
    }
    Ok(())
}

fn cmd_delete(indices: &[usize], range: Option<&[usize]>) -> Result<()> {
    let mut all_indices: Vec<usize> = indices.to_vec();
    if let Some(r) = range {
        if r.len() >= 2 {
            for i in r[0]..=r[1] {
                all_indices.push(i);
            }
        }
    }
    all_indices.sort();
    all_indices.dedup();

    let db = MeshletDb::open(&db_path()?)?;
    let bookmarks = db.list_from_mirror()?;

    for &idx in &all_indices {
        if idx < 1 || idx > bookmarks.len() {
            eprintln!(
                "{}: index {} out of range (have {} bookmarks)",
                "error".red(),
                idx,
                bookmarks.len()
            );
            continue;
        }
        let bookmark = &bookmarks[idx - 1];
        db.delete_bookmark(&bookmark.id)?;
        println!(
            "Deleted: {} — {}",
            idx.to_string().yellow(),
            bookmark.title.green()
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_edit(
    index: usize,
    url: Option<&str>,
    title: Option<&str>,
    tag_replace: Option<&str>,
    tag_add: Option<&str>,
    tag_delete: Option<&str>,
    desc: Option<&str>,
    immutable: Option<bool>,
) -> Result<()> {
    let db = MeshletDb::open(&db_path()?)?;
    let bookmarks = db.list_from_mirror()?;

    if index < 1 || index > bookmarks.len() {
        anyhow::bail!(
            "index {} out of range (have {} bookmarks)",
            index,
            bookmarks.len()
        );
    }

    let bookmark = &bookmarks[index - 1];

    let flags = immutable.map(|im| if im { 0x01 } else { 0 });

    let patch = BookmarkPatch {
        url: url.map(String::from),
        title: title.map(String::from),
        desc: desc.map(String::from),
        flags,
    };

    if patch.url.is_some() || patch.title.is_some() || patch.desc.is_some() {
        db.update_bookmark(&bookmark.id, &patch)?;
    }

    if let Some(replace) = tag_replace {
        let tags: BTreeSet<String> = parse_tags(replace);
        let existing: Vec<String> = bookmark.tags.iter().cloned().collect();
        db.remove_tags(&bookmark.id, &existing)?;
        let to_add: Vec<String> = tags.into_iter().collect();
        if !to_add.is_empty() {
            db.add_tags(&bookmark.id, &to_add)?;
        }
    }

    if let Some(add) = tag_add {
        let tags: Vec<String> = parse_tags(add).into_iter().collect();
        db.add_tags(&bookmark.id, &tags)?;
    }

    if let Some(delete) = tag_delete {
        let tags: Vec<String> = parse_tags(delete).into_iter().collect();
        db.remove_tags(&bookmark.id, &tags)?;
    }

    println!("Updated bookmark at index {}", index);
    Ok(())
}

fn cmd_tag(index: usize, tags: &[String], delete: bool) -> Result<()> {
    let db = MeshletDb::open(&db_path()?)?;
    let bookmarks = db.list_from_mirror()?;

    if index < 1 || index > bookmarks.len() {
        anyhow::bail!(
            "index {} out of range (have {} bookmarks)",
            index,
            bookmarks.len()
        );
    }

    let bookmark = &bookmarks[index - 1];
    let all_tags: Vec<String> = tags.iter().flat_map(|t| parse_tags(t)).collect();

    if delete {
        db.remove_tags(&bookmark.id, &all_tags)?;
        println!("Removed tags from '{}'", bookmark.title.green());
    } else {
        db.add_tags(&bookmark.id, &all_tags)?;
        println!("Added tags to '{}'", bookmark.title.green());
    }

    Ok(())
}

fn cmd_open(index: usize) -> Result<()> {
    let db = MeshletDb::open(&db_path()?)?;
    let bookmarks = db.list_from_mirror()?;

    if index < 1 || index > bookmarks.len() {
        anyhow::bail!(
            "index {} out of range (have {} bookmarks)",
            index,
            bookmarks.len()
        );
    }

    let bookmark = &bookmarks[index - 1];
    webbrowser::open(&bookmark.url).context("failed to open browser")?;
    println!("Opening: {}", bookmark.url.cyan());

    Ok(())
}

fn cmd_import(file: &str) -> Result<()> {
    let db = MeshletDb::open(&db_path()?)?;
    let path = std::path::Path::new(file);

    if !path.exists() {
        anyhow::bail!("file not found: {}", file);
    }

    let stats = import_export::import_netscape(path, &db)?;
    println!(
        "Imported {} bookmarks ({} skipped, {} total)",
        stats.imported, stats.skipped, stats.total
    );

    Ok(())
}

fn cmd_export(file: &str, format: &str) -> Result<()> {
    let db = MeshletDb::open(&db_path()?)?;
    let path = std::path::Path::new(file);

    match format.to_lowercase().as_str() {
        "html" => import_export::export_html(path, &db)?,
        "md" | "markdown" => import_export::export_markdown(path, &db)?,
        other => anyhow::bail!("unknown export format: {} (use 'html' or 'md')", other),
    }

    println!("Exported to {}", file);
    Ok(())
}

fn cmd_sync(server: Option<&str>, token: Option<&str>) -> Result<()> {
    use meshlet_proto::messages::{SyncRequest, SyncResponse};

    let config = load_config();
    let db = MeshletDb::open(&db_path()?)?;

    let server = server
        .or(config.server.url.as_deref())
        .context("no server URL provided (use --server or set in config.toml)")?;
    let token = token.or(config.server.token.as_deref());

    let last_vv = db.load_last_server_vv()?;

    let client_updates = if let Some(ref vv) = last_vv {
        db.export_updates_since(vv)?
    } else {
        db.export_updates_since(&loro::VersionVector::default())?
    };

    let client_vv = db.oplog_vv();
    let request = SyncRequest::new(&client_vv, &client_updates);

    let client = reqwest::blocking::Client::new();
    let mut builder = client
        .post(format!("{}/sync", server.trim_end_matches('/')))
        .json(&request);

    if let Some(t) = token {
        builder = builder.header("Authorization", format!("Bearer {}", t));
    }

    let response = builder
        .send()
        .context("failed to connect to sync server")?;

    if !response.status().is_success() {
        anyhow::bail!(
            "sync server returned {} — check your server URL and token",
            response.status()
        );
    }

    let sync_response: SyncResponse = response.json().context("invalid server response")?;

    let server_updates = sync_response.updates().context("invalid base64 in server response")?;
    let server_vv = sync_response.server_vv().context("invalid server VV")?;

    if !server_updates.is_empty() {
        let summary = db.sync_import(&server_updates)?;
        if summary.merged_duplicates > 0 {
            println!(
                "Synced from server — merged {} duplicate bookmarks",
                summary.merged_duplicates
            );
        } else {
            println!("Synced from server");
        }
    } else {
        println!("Already up to date.");
    }

    db.save_last_server_vv(&server_vv)?;

    Ok(())
}

fn cmd_sync_status() -> Result<()> {
    let db = MeshletDb::open(&db_path()?)?;
    match db.load_last_server_vv()? {
        Some(_vv) => {
            let count = db.list_from_mirror()?.len();
            println!("Last sync state loaded — {} bookmarks in local database", count);
        }
        None => {
            println!("Not yet synced with a server.");
        }
    }
    Ok(())
}

fn cmd_config(server: Option<&str>, token: Option<&str>, data_dir: Option<&str>) -> Result<()> {
    let config_path = config_dir()?.join("config.toml");

    let mut config = load_config();

    if let Some(s) = server {
        config.server.url = Some(s.to_string());
    }
    if let Some(t) = token {
        config.server.token = Some(t.to_string());
    }
    if let Some(d) = data_dir {
        config.data_dir = Some(d.to_string());
    }

    if server.is_none() && token.is_none() && data_dir.is_none() {
        println!("Config file: {:?}", config_path);
        println!("Server URL: {}", config.server.url.as_deref().unwrap_or("(not set)"));
        println!("Token: {}", if config.server.token.is_some() { "(set)" } else { "(not set)" });
        println!(
            "Data dir: {}",
            config.data_dir.as_deref().unwrap_or("(default: ~/.local/share/meshlet)")
        );
        return Ok(());
    }

    let content = toml::to_string_pretty(&config).context("failed to serialize config")?;
    std::fs::write(&config_path, content).context("failed to write config file")?;
    println!("Config written to {:?}", config_path);

    Ok(())
}

fn cmd_gc() -> Result<()> {
    let db = MeshletDb::open(&db_path()?)?;
    db.compact_change_store();
    println!("Garbage collection complete");
    Ok(())
}

fn display_bookmarks(bookmarks: &[Bookmark], config: &DisplayConfig) -> String {
    use std::fmt::Write;

    let show_url = config.show_url.unwrap_or(true);
    let show_desc = config.show_desc.unwrap_or(true);
    let show_tags = config.show_tags.unwrap_or(true);

    let mut out = String::new();
    for (i, bm) in bookmarks.iter().enumerate() {
        let idx = i + 1;
        let tag_str = if show_tags && !bm.tags.is_empty() {
            format!(
                " [{}]",
                bm.tags
                    .iter()
                    .map(|t| t.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else {
            String::new()
        };

        writeln!(
            out,
            " {:>4}. {}{}",
            idx.to_string().yellow(),
            bm.title.green(),
            tag_str.magenta()
        )
        .unwrap();

        if show_url && !bm.url.is_empty() {
            writeln!(out, "      > {}", bm.url.cyan()).unwrap();
        }
        if show_desc && !bm.desc.is_empty() {
            writeln!(out, "      + {}", bm.desc.yellow()).unwrap();
        }
        if show_tags && !bm.tags.is_empty() {
            writeln!(
                out,
                "      # {}",
                bm.tags
                    .iter()
                    .map(|t| t.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
                    .magenta()
            )
            .unwrap();
        }
    }
    out
}

fn parse_tags(input: &str) -> BTreeSet<String> {
    input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn sample_bookmarks() -> Vec<Bookmark> {
        vec![
            Bookmark {
                id: BookmarkId("01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
                url: "https://loro.dev".into(),
                title: "Loro CRDT".into(),
                desc: "A high-performance CRDT framework".into(),
                tags: ["crdt", "rust"].iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(),
                flags: 0,
                created_at: 1719000000,
                updated_at: 1719100000,
            },
            Bookmark {
                id: BookmarkId("01ARZ3NDEKTSV4RRFFQ69G5FB0".into()),
                url: "https://rust-lang.org".into(),
                title: "Rust".into(),
                desc: String::new(),
                tags: ["lang"].iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(),
                flags: 1,
                created_at: 1718000000,
                updated_at: 1718100000,
            },
            Bookmark {
                id: BookmarkId("01ARZ3NDEKTSV4RRFFQ69G5FB1".into()),
                url: "https://example.com".into(),
                title: "No Tags".into(),
                desc: "Just a URL and description".into(),
                tags: BTreeSet::new(),
                flags: 0,
                created_at: 1717000000,
                updated_at: 1717100000,
            },
        ]
    }

    #[test]
    fn snapshot_display_output() {
        colored::control::set_override(false);
        let config = DisplayConfig {
            color: Some(false),
            show_url: Some(true),
            show_desc: Some(true),
            show_tags: Some(true),
        };
        let output = display_bookmarks(&sample_bookmarks(), &config);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn snapshot_display_output_minimal() {
        colored::control::set_override(false);
        let config = DisplayConfig {
            show_url: Some(false),
            show_desc: Some(false),
            show_tags: Some(false),
            ..Default::default()
        };
        let output = display_bookmarks(&sample_bookmarks(), &config);
        insta::assert_snapshot!(output);
    }
}