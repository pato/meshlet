use std::collections::BTreeSet;

use rusqlite::{Connection, params};

use crate::error::Result;
use crate::model::{Bookmark, BookmarkId};

pub fn list_all(conn: &Connection) -> Result<Vec<Bookmark>> {
    let mut stmt = conn.prepare(
        "SELECT id, url, title, desc, immutable_title, created_at, updated_at
         FROM bookmarks
         ORDER BY created_at ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let url: String = row.get(1)?;
        let title: String = row.get(2)?;
        let desc: String = row.get(3)?;
        let immutable: i64 = row.get(4)?;
        let created_at: i64 = row.get(5)?;
        let updated_at: i64 = row.get(6)?;
        Ok((id, url, title, desc, immutable, created_at, updated_at))
    })?;

    let mut bookmarks = Vec::new();
    for row in rows {
        let (id, url, title, desc, immutable, created_at, updated_at) = row?;
        let tags = get_tags(conn, &id)?;
        bookmarks.push(Bookmark {
            id: BookmarkId(id),
            url,
            title,
            desc,
            tags,
            flags: immutable,
            created_at,
            updated_at,
        });
    }

    Ok(bookmarks)
}

pub fn search_keywords(
    conn: &Connection,
    keywords: &[String],
    deep: bool,
    all_match: bool,
) -> Result<Vec<Bookmark>> {
    if keywords.is_empty() {
        return list_all(conn);
    }

    let mut conditions = Vec::new();
    let mut params_list: Vec<String> = Vec::new();

    for (i, kw) in keywords.iter().enumerate() {
        let param_name = format!("kw{}", i);
        if deep {
            conditions.push(format!(
                "(url LIKE '%' || :{} || '%' OR title LIKE '%' || :{} || '%' OR desc LIKE '%' || :{} || '%' OR id IN (SELECT bookmark_id FROM bookmark_tags WHERE tag LIKE '%' || :{} || '%'))",
                param_name, param_name, param_name, param_name
            ));
        } else {
            conditions.push(format!(
                "(url LIKE :{} OR title LIKE :{} OR desc LIKE :{} OR id IN (SELECT bookmark_id FROM bookmark_tags WHERE tag LIKE :{}))",
                param_name, param_name, param_name, param_name
            ));
        }
        params_list.push(format!("%{}%", kw));
    }

    let operator = if all_match { " AND " } else { " OR " };
    let where_clause = conditions.join(operator);

    let sql = format!(
        "SELECT id, url, title, desc, immutable_title, created_at, updated_at
         FROM bookmarks
         WHERE {}
         ORDER BY created_at ASC",
        where_clause
    );

    let mut stmt = conn.prepare(&sql)?;

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params_list
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let id: String = row.get(0)?;
        let url: String = row.get(1)?;
        let title: String = row.get(2)?;
        let desc: String = row.get(3)?;
        let immutable: i64 = row.get(4)?;
        let created_at: i64 = row.get(5)?;
        let updated_at: i64 = row.get(6)?;
        Ok((id, url, title, desc, immutable, created_at, updated_at))
    })?;

    let mut bookmarks = Vec::new();
    for row in rows {
        let (id, url, title, desc, immutable, created_at, updated_at) = row?;
        let tags = get_tags(conn, &id)?;
        bookmarks.push(Bookmark {
            id: BookmarkId(id),
            url,
            title,
            desc,
            tags,
            flags: immutable,
            created_at,
            updated_at,
        });
    }

    Ok(bookmarks)
}

pub fn search_by_tags(conn: &Connection, tags: &[String]) -> Result<Vec<Bookmark>> {
    if tags.is_empty() {
        return list_all(conn);
    }

    let placeholders: Vec<String> = (0..tags.len()).map(|i| format!("?{}", i + 1)).collect();
    let sql = format!(
        "SELECT id, url, title, desc, immutable_title, created_at, updated_at
         FROM bookmarks
         WHERE id IN (SELECT bookmark_id FROM bookmark_tags WHERE tag IN ({}))
         ORDER BY created_at ASC",
        placeholders.join(", ")
    );

    let mut stmt = conn.prepare(&sql)?;

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = tags
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let id: String = row.get(0)?;
        let url: String = row.get(1)?;
        let title: String = row.get(2)?;
        let desc: String = row.get(3)?;
        let immutable: i64 = row.get(4)?;
        let created_at: i64 = row.get(5)?;
        let updated_at: i64 = row.get(6)?;
        Ok((id, url, title, desc, immutable, created_at, updated_at))
    })?;

    let mut bookmarks = Vec::new();
    for row in rows {
        let (id, url, title, desc, immutable, created_at, updated_at) = row?;
        let tags = get_tags(conn, &id)?;
        bookmarks.push(Bookmark {
            id: BookmarkId(id),
            url,
            title,
            desc,
            tags,
            flags: immutable,
            created_at,
            updated_at,
        });
    }

    Ok(bookmarks)
}

pub fn search_regex(
    conn: &Connection,
    pattern: &str,
    field: Option<&str>,
) -> Result<Vec<Bookmark>> {
    let sql = match field {
        Some("url") => {
            "SELECT id, url, title, desc, immutable_title, created_at, updated_at
             FROM bookmarks WHERE regexp(?1, url) ORDER BY created_at ASC"
        }
        Some("title") => {
            "SELECT id, url, title, desc, immutable_title, created_at, updated_at
             FROM bookmarks WHERE regexp(?1, title) ORDER BY created_at ASC"
        }
        _ => {
            "SELECT id, url, title, desc, immutable_title, created_at, updated_at
             FROM bookmarks WHERE regexp(?1, url) OR regexp(?1, title) OR regexp(?1, desc)
             ORDER BY created_at ASC"
        }
    };

    let mut stmt = conn.prepare(sql)?;

    let rows = stmt.query_map(params![pattern], |row| {
        let id: String = row.get(0)?;
        let url: String = row.get(1)?;
        let title: String = row.get(2)?;
        let desc: String = row.get(3)?;
        let immutable: i64 = row.get(4)?;
        let created_at: i64 = row.get(5)?;
        let updated_at: i64 = row.get(6)?;
        Ok((id, url, title, desc, immutable, created_at, updated_at))
    })?;

    let mut bookmarks = Vec::new();
    for row in rows {
        let (id, url, title, desc, immutable, created_at, updated_at) = row?;
        let tags = get_tags(conn, &id)?;
        bookmarks.push(Bookmark {
            id: BookmarkId(id),
            url,
            title,
            desc,
            tags,
            flags: immutable,
            created_at,
            updated_at,
        });
    }

    Ok(bookmarks)
}

fn get_tags(conn: &Connection, bookmark_id: &str) -> Result<BTreeSet<String>> {
    let mut stmt = conn.prepare("SELECT tag FROM bookmark_tags WHERE bookmark_id = ?1")?;

    let tags = stmt.query_map(params![bookmark_id], |row| row.get::<_, String>(0))?;

    let mut set = BTreeSet::new();
    for tag in tags {
        set.insert(tag?);
    }
    Ok(set)
}