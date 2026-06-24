use std::path::Path;

use rusqlite::{Connection, OpenFlags, params};

use crate::error::Result;

const SCHEMA_VERSION: i32 = 1;

static CREATE_LORO_DOC: &str = "
CREATE TABLE IF NOT EXISTS loro_doc (
    id       INTEGER PRIMARY KEY,
    snapshot BLOB NOT NULL,
    vv       BLOB NOT NULL
);
";

static CREATE_BOOKMARKS: &str = "
CREATE TABLE IF NOT EXISTS bookmarks (
    id              TEXT PRIMARY KEY,
    url             TEXT NOT NULL,
    title           TEXT NOT NULL DEFAULT '',
    desc            TEXT NOT NULL DEFAULT '',
    immutable_title INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER,
    updated_at      INTEGER
);
";

static CREATE_BOOKMARK_TAGS: &str = "
CREATE TABLE IF NOT EXISTS bookmark_tags (
    bookmark_id TEXT NOT NULL REFERENCES bookmarks(id) ON DELETE CASCADE,
    tag         TEXT NOT NULL,
    PRIMARY KEY (bookmark_id, tag)
);
";

static CREATE_INDEX_TAG: &str =
    "CREATE INDEX IF NOT EXISTS idx_bookmark_tags_tag ON bookmark_tags(tag);";
static CREATE_INDEX_URL: &str =
    "CREATE INDEX IF NOT EXISTS idx_bookmarks_url ON bookmarks(url);";

static CREATE_META: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value BLOB NOT NULL
);
";

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let store = Self { conn };
        store.migrate()?;
        store.register_functions()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let store = Self { conn };
        store.migrate()?;
        store.register_functions()?;
        Ok(store)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn save_snapshot(&self, snapshot: &[u8], vv: &[u8]) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO loro_doc (id, snapshot, vv) VALUES (1, ?1, ?2)",
            params![snapshot, vv],
        )?;
        Ok(())
    }

    pub fn load_snapshot(&self) -> Result<Option<Vec<u8>>> {
        let mut stmt = self
            .conn
            .prepare("SELECT snapshot FROM loro_doc WHERE id = 1")?;

        let result = stmt.query_row([], |row| row.get(0)).optional()?;
        Ok(result)
    }

    pub fn load_vv(&self) -> Result<Option<Vec<u8>>> {
        let mut stmt = self
            .conn
            .prepare("SELECT vv FROM loro_doc WHERE id = 1")?;

        let result = stmt.query_row([], |row| row.get(0)).optional()?;
        Ok(result)
    }

    pub fn set_meta(&self, key: &str, value: &[u8]) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM meta WHERE key = ?1")?;

        let result = stmt
            .query_row(params![key], |row| row.get(0))
            .optional()?;
        Ok(result)
    }

    pub fn set_meta_string(&self, key: &str, value: &str) -> Result<()> {
        self.set_meta(key, value.as_bytes())
    }

    pub fn get_meta_string(&self, key: &str) -> Result<Option<String>> {
        match self.get_meta(key)? {
            Some(data) => Ok(String::from_utf8(data).ok()),
            None => Ok(None),
        }
    }

    pub fn upsert_bookmark(&self, b: &crate::model::Bookmark) -> Result<()> {
        self.conn.execute(
            "INSERT INTO bookmarks (id, url, title, desc, immutable_title, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
               url = excluded.url,
               title = excluded.title,
               desc = excluded.desc,
               immutable_title = excluded.immutable_title,
               updated_at = excluded.updated_at",
            params![
                b.id.as_str(),
                b.url,
                b.title,
                b.desc,
                b.flags & 0x01,
                b.created_at,
                b.updated_at
            ],
        )?;

        self.replace_tags(b.id.as_str(), &b.tags)?;
        Ok(())
    }

    pub fn delete_bookmark_mirror(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM bookmarks WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn replace_tags(
        &self,
        bookmark_id: &str,
        tags: &std::collections::BTreeSet<String>,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            "DELETE FROM bookmark_tags WHERE bookmark_id = ?1",
            params![bookmark_id],
        )?;

        let mut stmt =
            tx.prepare("INSERT INTO bookmark_tags (bookmark_id, tag) VALUES (?1, ?2)")?;

        for tag in tags {
            stmt.execute(params![bookmark_id, tag])?;
        }

        drop(stmt);
        tx.commit()?;
        Ok(())
    }

    fn migrate(&self) -> Result<()> {
        let current_version: i32 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if current_version < 1 {
            let tx = self.conn.unchecked_transaction()?;

            tx.execute_batch(CREATE_META)?;
            tx.execute_batch(CREATE_LORO_DOC)?;
            tx.execute_batch(CREATE_BOOKMARKS)?;
            tx.execute_batch(CREATE_BOOKMARK_TAGS)?;
            tx.execute_batch(CREATE_INDEX_TAG)?;
            tx.execute_batch(CREATE_INDEX_URL)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;

            tx.commit()?;
        }

        Ok(())
    }

    fn register_functions(&self) -> Result<()> {
        use rusqlite::functions::FunctionFlags;

        self.conn.create_scalar_function(
            "netloc",
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |ctx| {
                let s = ctx.get_raw(0).as_str().unwrap_or("");
                if let Ok(parsed) = url::Url::parse(s) {
                    match parsed.host_str() {
                        Some(host) => Ok(host.to_string()),
                        None => Ok(String::new()),
                    }
                } else {
                    Ok(String::new())
                }
            },
        )?;

        self.conn.create_scalar_function(
            "regexp",
            2,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |ctx| {
                let pattern = ctx.get_raw(0).as_str().unwrap_or("");
                let text = ctx.get_raw(1).as_str().unwrap_or("");
                match regex::Regex::new(pattern) {
                    Ok(re) => Ok(re.is_match(text) as i32),
                    Err(_) => Ok(0),
                }
            },
        )?;

        Ok(())
    }
}

trait OptionalExt<T> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for std::result::Result<T, rusqlite::Error> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}