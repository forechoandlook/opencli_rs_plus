//! Adapter index: SQLite FTS5 (BM25) + usage statistics.
//!
//! DB file: ~/.opencli-rs/index.db
//! Tables:
//!   adapter_fts        — FTS5 virtual table
//!   adapter_usage      — cumulative usage counters
//!   adapter_index_meta — per-adapter mtime, used for incremental sync

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::adapter_manager::AdapterEntry;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub full_name: String,
    pub site: String,
    pub name: String,
    pub description: String,
    pub domain: Option<String>,
    pub browser: bool,
    pub score: f64,
    pub usage_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotAdapter {
    pub full_name: String,
    pub site: String,
    pub name: String,
    pub description: String,
    pub usage_count: i64,
    pub last_used: Option<i64>,
}

/// Summary of what changed during a sync.
#[derive(Debug, Default)]
pub struct SyncStats {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
}

// ── Store ─────────────────────────────────────────────────────────────────────

pub struct AdapterIndex {
    conn: Mutex<Connection>,
}

impl AdapterIndex {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&db_path)?;

        conn.execute_batch(
            "
            CREATE VIRTUAL TABLE IF NOT EXISTS adapter_fts USING fts5(
                full_name,
                site,
                name,
                description,
                domain,
                summary,
                tokenize = 'unicode61'
            );

            CREATE TABLE IF NOT EXISTS adapter_usage (
                full_name   TEXT PRIMARY KEY,
                site        TEXT NOT NULL,
                name        TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                count       INTEGER NOT NULL DEFAULT 0,
                last_used   INTEGER
            );

            -- Tracks mtime of each adapter's yaml + summary files.
            -- Used to skip unchanged adapters during incremental sync.
            CREATE TABLE IF NOT EXISTS adapter_index_meta (
                full_name     TEXT PRIMARY KEY,
                yaml_mtime    INTEGER NOT NULL DEFAULT 0,
                summary_mtime INTEGER NOT NULL DEFAULT 0,
                indexed_at    INTEGER NOT NULL
            );
            ",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Incremental sync: only re-index adapters whose yaml or summary has changed.
    /// Also removes entries for adapters that no longer exist.
    /// Returns stats about what changed.
    pub fn sync(&self, adapters: &[AdapterEntry]) -> Result<SyncStats> {
        let mut stats = SyncStats::default();
        let now = unix_now();

        // Build lookup: full_name → AdapterEntry
        let current: HashMap<&str, &AdapterEntry> =
            adapters.iter().map(|a| (a.full_name.as_str(), a)).collect();

        let conn = self.conn.lock().unwrap();

        // Load existing meta: full_name → (yaml_mtime, summary_mtime)
        let existing_meta: HashMap<String, (i64, i64)> = {
            let mut stmt = conn
                .prepare("SELECT full_name, yaml_mtime, summary_mtime FROM adapter_index_meta")?;
            let rows: Vec<_> = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        (row.get::<_, i64>(1)?, row.get::<_, i64>(2)?),
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();
            rows.into_iter().collect()
        };

        // Prepare statements
        let mut fts_delete = conn.prepare("DELETE FROM adapter_fts WHERE full_name = ?1")?;
        let mut fts_insert = conn.prepare(
            "INSERT INTO adapter_fts(full_name, site, name, description, domain, summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        let mut meta_upsert = conn.prepare(
            "INSERT INTO adapter_index_meta(full_name, yaml_mtime, summary_mtime, indexed_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(full_name) DO UPDATE SET
                yaml_mtime    = excluded.yaml_mtime,
                summary_mtime = excluded.summary_mtime,
                indexed_at    = excluded.indexed_at",
        )?;

        // 1. Add / update changed adapters
        for a in adapters {
            let yaml_mtime = yaml_file_mtime(&a.site, &a.name);
            let (summary_text, summary_mtime) = read_summary_with_mtime(&a.site, &a.name);

            let needs_update = match existing_meta.get(&a.full_name) {
                None => true, // new
                Some(&(prev_yaml, prev_summary)) => {
                    prev_yaml != yaml_mtime || prev_summary != summary_mtime
                }
            };

            if needs_update {
                // Remove stale FTS row (if any) and re-insert
                fts_delete.execute(params![a.full_name])?;
                fts_insert.execute(params![
                    a.full_name,
                    a.site,
                    a.name,
                    a.description,
                    a.domain.as_deref().unwrap_or(""),
                    summary_text,
                ])?;
                meta_upsert.execute(params![a.full_name, yaml_mtime, summary_mtime, now])?;

                if existing_meta.contains_key(&a.full_name) {
                    stats.updated += 1;
                } else {
                    stats.added += 1;
                }
            } else {
                stats.unchanged += 1;
            }
        }

        // 2. Remove adapters that no longer exist
        let stale: Vec<String> = existing_meta
            .keys()
            .filter(|name| !current.contains_key(name.as_str()))
            .cloned()
            .collect();

        for name in &stale {
            fts_delete.execute(params![name])?;
            conn.execute(
                "DELETE FROM adapter_index_meta WHERE full_name = ?1",
                params![name],
            )?;
            stats.removed += 1;
        }

        tracing::info!(
            added = stats.added,
            updated = stats.updated,
            removed = stats.removed,
            unchanged = stats.unchanged,
            "FTS index synced"
        );
        Ok(stats)
    }

    /// Full rebuild — clears everything and re-indexes all adapters.
    /// Use this when the DB might be corrupt or on first run after schema change.
    pub fn rebuild(&self, adapters: &[AdapterEntry]) -> Result<()> {
        let now = unix_now();
        let conn = self.conn.lock().unwrap();

        conn.execute_batch("DELETE FROM adapter_fts; DELETE FROM adapter_index_meta;")?;

        let mut fts_insert = conn.prepare(
            "INSERT INTO adapter_fts(full_name, site, name, description, domain, summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        let mut meta_insert = conn.prepare(
            "INSERT INTO adapter_index_meta(full_name, yaml_mtime, summary_mtime, indexed_at)
             VALUES (?1, ?2, ?3, ?4)",
        )?;

        for a in adapters {
            let yaml_mtime = yaml_file_mtime(&a.site, &a.name);
            let (summary_text, summary_mtime) = read_summary_with_mtime(&a.site, &a.name);

            fts_insert.execute(params![
                a.full_name,
                a.site,
                a.name,
                a.description,
                a.domain.as_deref().unwrap_or(""),
                summary_text,
            ])?;
            meta_insert.execute(params![a.full_name, yaml_mtime, summary_mtime, now])?;
        }

        tracing::info!(count = adapters.len(), "FTS index fully rebuilt");
        Ok(())
    }

    /// BM25 + usage hybrid search.
    /// score = 0.7 × BM25 + 0.3 × log(1 + usage_count)
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        if query.trim().is_empty() {
            return Ok(vec![]);
        }

        let safe_query = sanitize_fts_query(query);
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT
                f.full_name,
                f.site,
                f.name,
                f.description,
                f.domain,
                -bm25(adapter_fts) AS bm25_score,
                COALESCE(u.count, 0) AS usage_count
             FROM adapter_fts f
             LEFT JOIN adapter_usage u ON f.full_name = u.full_name
             WHERE adapter_fts MATCH ?1
             ORDER BY (-bm25(adapter_fts) * 0.7 + LOG(1.0 + COALESCE(u.count, 0)) * 0.3) DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![safe_query, limit as i64], |row| {
            Ok(SearchResult {
                full_name: row.get(0)?,
                site: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                domain: {
                    let d: String = row.get(4)?;
                    if d.is_empty() {
                        None
                    } else {
                        Some(d)
                    }
                },
                browser: false,
                score: row.get(5)?,
                usage_count: row.get(6)?,
            })
        })?;

        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Record a successful adapter execution.
    pub fn record_usage(
        &self,
        full_name: &str,
        site: &str,
        name: &str,
        description: &str,
    ) -> Result<()> {
        let now = unix_now();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO adapter_usage(full_name, site, name, description, count, last_used)
             VALUES (?1, ?2, ?3, ?4, 1, ?5)
             ON CONFLICT(full_name) DO UPDATE SET
                count = count + 1,
                last_used = excluded.last_used",
            params![full_name, site, name, description, now],
        )?;
        Ok(())
    }

    /// Top-N adapters by cumulative usage.
    pub fn hot(&self, limit: usize) -> Result<Vec<HotAdapter>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT full_name, site, name, description, count, last_used
             FROM adapter_usage ORDER BY count DESC LIMIT ?1",
        )?;
        query_hot(&mut stmt, params![limit as i64])
    }

    /// Adapters active within the last `days` days.
    pub fn trending(&self, days: i64, limit: usize) -> Result<Vec<HotAdapter>> {
        let since = unix_now() - days * 86400;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT full_name, site, name, description, count, last_used
             FROM adapter_usage
             WHERE last_used >= ?1
             ORDER BY count DESC, last_used DESC
             LIMIT ?2",
        )?;
        query_hot(&mut stmt, params![since, limit as i64])
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn query_hot(
    stmt: &mut rusqlite::Statement<'_>,
    params: impl rusqlite::Params,
) -> Result<Vec<HotAdapter>> {
    let rows = stmt.query_map(params, |row| {
        Ok(HotAdapter {
            full_name: row.get(0)?,
            site: row.get(1)?,
            name: row.get(2)?,
            description: row.get(3)?,
            usage_count: row.get(4)?,
            last_used: row.get(5)?,
        })
    })?;
    rows.map(|r| r.map_err(Into::into)).collect()
}

/// Get mtime of the yaml file for a given adapter.
/// Checks both local `adapters/` and `~/.opencli-rs/adapters/`. Returns 0 if not found.
fn yaml_file_mtime(site: &str, name: &str) -> i64 {
    let candidates = [
        PathBuf::from("adapters")
            .join(site)
            .join(format!("{}.yaml", name)),
        dirs::home_dir()
            .map(|h| {
                h.join(".opencli-rs")
                    .join("adapters")
                    .join(site)
                    .join(format!("{}.yaml", name))
            })
            .unwrap_or_default(),
    ];
    for path in &candidates {
        if let Ok(meta) = std::fs::metadata(path) {
            if let Ok(mtime) = meta.modified() {
                return mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
            }
        }
    }
    0
}

/// Read summary text and return (content, mtime). mtime is 0 if no file found.
fn read_summary_with_mtime(site: &str, name: &str) -> (String, i64) {
    let candidates = [
        PathBuf::from("adapters").join(site).join("summary.md"),
        PathBuf::from("adapters")
            .join(site)
            .join(format!("{}.md", name)),
        PathBuf::from("summaries").join(format!("{}-{}.md", site, name)),
        PathBuf::from("summaries").join(format!("{}.md", site)),
        dirs::home_dir()
            .map(|h| {
                h.join(".opencli-rs")
                    .join("adapters")
                    .join(site)
                    .join("summary.md")
            })
            .unwrap_or_default(),
        dirs::home_dir()
            .map(|h| {
                h.join(".opencli-rs")
                    .join("summaries")
                    .join(format!("{}-{}.md", site, name))
            })
            .unwrap_or_default(),
        dirs::home_dir()
            .map(|h| {
                h.join(".opencli-rs")
                    .join("summaries")
                    .join(format!("{}.md", site))
            })
            .unwrap_or_default(),
    ];
    for path in &candidates {
        if let Ok(meta) = std::fs::metadata(path) {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if let Ok(content) = std::fs::read_to_string(path) {
                return (content, mtime);
            }
        }
    }
    (String::new(), 0)
}

/// Sanitize user input for FTS5 MATCH: prefix-match each token.
fn sanitize_fts_query(q: &str) -> String {
    let terms: Vec<String> = q
        .split_whitespace()
        .filter_map(|t| {
            let clean: String = t
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if clean.is_empty() {
                None
            } else {
                Some(format!("\"{}\"*", clean))
            }
        })
        .collect();
    if terms.is_empty() {
        String::from("\"\"")
    } else {
        terms.join(" ")
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
