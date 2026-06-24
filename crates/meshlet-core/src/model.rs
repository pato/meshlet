use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct BookmarkId(pub String);

impl BookmarkId {
    pub fn new() -> Self {
        Self(ulid::Ulid::new().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for BookmarkId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for BookmarkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for BookmarkId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for BookmarkId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bookmark {
    pub id: BookmarkId,
    pub url: String,
    pub title: String,
    pub desc: String,
    pub tags: BTreeSet<String>,
    pub flags: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct BookmarkPatch {
    pub url: Option<String>,
    pub title: Option<String>,
    pub desc: Option<String>,
    pub flags: Option<i64>,
}