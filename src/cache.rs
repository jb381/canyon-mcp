use crate::{CanyonError, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

#[derive(Debug)]
pub struct CacheEntry {
    pub body: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub fetched_at: String,
}

#[derive(Debug, Serialize)]
pub struct CacheStats {
    pub path: PathBuf,
    pub entries: i64,
    pub bytes: i64,
}

pub struct HttpCache {
    path: PathBuf,
    db: Mutex<Connection>,
}

pub fn default_cache_path() -> PathBuf {
    env::var_os("CANYON_CACHE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".cache/canyon-mcp/cache.sqlite")
        })
}

fn cache_error(error: impl std::fmt::Display) -> CanyonError {
    CanyonError::new("CACHE_ERROR", format!("HTTP cache: {error}"))
}

impl HttpCache {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(cache_error)?;
        }
        let db = Connection::open(path).map_err(cache_error)?;
        db.busy_timeout(Duration::from_secs(5))
            .map_err(cache_error)?;
        // Keep the Bun schema so existing files and rollback images can both use it.
        let mode: String = db
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(cache_error)?;
        if mode != "wal" && path != Path::new(":memory:") {
            db.pragma_update(None, "journal_mode", "WAL")
                .map_err(cache_error)?;
        }
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS http_cache (
                url TEXT PRIMARY KEY,
                body TEXT NOT NULL,
                etag TEXT,
                last_modified TEXT,
                fetched_at TEXT NOT NULL
            );",
        )
        .map_err(cache_error)?;
        Ok(Self {
            path: path.into(),
            db: Mutex::new(db),
        })
    }

    fn db(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.db.lock().map_err(cache_error)
    }

    pub fn get(&self, url: &str) -> Result<Option<CacheEntry>> {
        self.db()?
            .prepare_cached(
                "SELECT body, etag, last_modified, fetched_at FROM http_cache WHERE url = ?1",
            )
            .and_then(|mut stmt| {
                stmt.query_row([url], |row| {
                    Ok(CacheEntry {
                        body: row.get(0)?,
                        etag: row.get(1)?,
                        last_modified: row.get(2)?,
                        fetched_at: row.get(3)?,
                    })
                })
                .optional()
            })
            .map_err(cache_error)
    }

    pub fn put(&self, url: &str, entry: &CacheEntry) -> Result<()> {
        self.db()?
            .prepare_cached(
                "INSERT INTO http_cache (url, body, etag, last_modified, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(url) DO UPDATE SET
                body = excluded.body, etag = excluded.etag,
                last_modified = excluded.last_modified, fetched_at = excluded.fetched_at",
            )
            .and_then(|mut stmt| {
                stmt.execute(params![
                    url,
                    entry.body,
                    entry.etag,
                    entry.last_modified,
                    entry.fetched_at
                ])
            })
            .map_err(cache_error)?;
        Ok(())
    }

    pub(crate) fn touch(&self, url: &str, previous: &CacheEntry, fetched_at: &str) -> Result<bool> {
        self.db()?
            .prepare_cached(
                "UPDATE http_cache SET fetched_at = ?1
             WHERE url = ?2 AND fetched_at = ?3 AND etag IS ?4 AND last_modified IS ?5",
            )
            .and_then(|mut stmt| {
                stmt.execute(params![
                    fetched_at,
                    url,
                    previous.fetched_at,
                    previous.etag,
                    previous.last_modified
                ])
            })
            .map(|changed| changed == 1)
            .map_err(cache_error)
    }

    pub fn stats(&self) -> Result<CacheStats> {
        let (entries, bytes) = self
            .db()?
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(LENGTH(body)), 0) FROM http_cache",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(cache_error)?;
        Ok(CacheStats {
            path: self.path.clone(),
            entries,
            bytes,
        })
    }
}
