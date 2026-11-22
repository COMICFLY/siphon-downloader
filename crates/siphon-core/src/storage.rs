use crate::error::Result;
use crate::model::{
    ChecksumSpec, DownloadTask, FileCategory, ProtocolKind, SegmentInfo, SegmentState, TaskId,
    TaskLogEntry, TaskSource, TaskStatus,
};
use crate::rules::{AutomationRule, RuleEngine};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

pub struct TaskStore {
    conn: Mutex<Connection>,
}

impl TaskStore {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                protocol TEXT NOT NULL,
                original_url TEXT NOT NULL,
                resolved_url TEXT NOT NULL,
                filename TEXT NOT NULL,
                save_dir TEXT NOT NULL,
                total_size INTEGER,
                downloaded_bytes INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                supports_range INTEGER NOT NULL DEFAULT 0,
                etag TEXT,
                last_modified TEXT,
                content_type TEXT,
                headers_json TEXT NOT NULL DEFAULT '{}',
                max_connections INTEGER NOT NULL DEFAULT 8,
                speed_limit_bps INTEGER NOT NULL DEFAULT 0,
                category TEXT NOT NULL DEFAULT 'other',
                matched_rule TEXT,
                checksum_json TEXT,
                source TEXT NOT NULL DEFAULT 'manual',
                error_message TEXT,
                created_at TEXT NOT NULL,
                completed_at TEXT
            );

            CREATE TABLE IF NOT EXISTS segments (
                task_id TEXT NOT NULL,
                seg_index INTEGER NOT NULL,
                start_offset INTEGER NOT NULL,
                current_offset INTEGER NOT NULL,
                end_offset INTEGER NOT NULL,
                state TEXT NOT NULL,
                PRIMARY KEY (task_id, seg_index),
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS rules (
                id TEXT PRIMARY KEY,
                priority INTEGER NOT NULL,
                rule_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS history_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                level TEXT NOT NULL,
                message TEXT NOT NULL,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );
            "#,
        )?;
        Ok(())
    }

    /// Recover gracefully from an unexpected crash: active states become `Paused`.
    pub fn recover_interrupted_tasks(&self) -> Result<usize> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let changed = conn.execute(
            "UPDATE tasks SET status = 'paused' WHERE status IN ('downloading', 'probing', 'verifying')",
            [],
        )?;
        conn.execute(
            "UPDATE segments SET state = 'pending' WHERE state IN ('active', 'retrying')",
            [],
        )?;
        Ok(changed)
    }

    pub fn upsert_task(&self, task: &DownloadTask) -> Result<()> {
        let mut conn = self.conn.lock().expect("sqlite lock poisoned");
        let tx = conn.transaction()?;

        let headers_json = serde_json::to_string(&task.headers)?;
        let checksum_json = task
            .checksum
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let source_str = serde_json::to_string(&task.source)?;

        tx.execute(
            r#"
            INSERT INTO tasks (
                id, protocol, original_url, resolved_url, filename, save_dir,
                total_size, downloaded_bytes, status, supports_range,
                etag, last_modified, content_type, headers_json,
                max_connections, speed_limit_bps, category, matched_rule,
                checksum_json, source, error_message, created_at, completed_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14,
                ?15, ?16, ?17, ?18,
                ?19, ?20, ?21, ?22, ?23
            )
            ON CONFLICT(id) DO UPDATE SET
                protocol = excluded.protocol,
                original_url = excluded.original_url,
                resolved_url = excluded.resolved_url,
                filename = excluded.filename,
                save_dir = excluded.save_dir,
                total_size = excluded.total_size,
                downloaded_bytes = excluded.downloaded_bytes,
                status = excluded.status,
                supports_range = excluded.supports_range,
                etag = excluded.etag,
                last_modified = excluded.last_modified,
                content_type = excluded.content_type,
                headers_json = excluded.headers_json,
                max_connections = excluded.max_connections,
                speed_limit_bps = excluded.speed_limit_bps,
                category = excluded.category,
                matched_rule = excluded.matched_rule,
                checksum_json = excluded.checksum_json,
                source = excluded.source,
                error_message = excluded.error_message,
                completed_at = excluded.completed_at
            "#,
            params![
                task.id.to_string(),
                task.protocol.as_str(),
                task.original_url,
                task.resolved_url,
                task.filename,
                task.save_dir.to_string_lossy().to_string(),
                task.total_size.map(|v| v as i64),
                task.downloaded_bytes as i64,
                task.status.as_str(),
                if task.supports_range { 1i64 } else { 0i64 },
                task.etag,
                task.last_modified,
                task.content_type,
                headers_json,
                task.max_connections as i64,
                task.speed_limit_bps as i64,
                task.category.as_str(),
                task.matched_rule,
                checksum_json,
                source_str,
                task.error_message,
                task.created_at.to_rfc3339(),
                task.completed_at.map(|t| t.to_rfc3339()),
            ],
        )?;

        tx.execute(
            "DELETE FROM segments WHERE task_id = ?1",
            params![task.id.to_string()],
        )?;
        for seg in &task.segments {
            tx.execute(
                "INSERT INTO segments (task_id, seg_index, start_offset, current_offset, end_offset, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    task.id.to_string(),
                    seg.index as i64,
                    seg.start_offset as i64,
                    seg.current_offset as i64,
                    if seg.end_offset == u64::MAX { -1i64 } else { seg.end_offset as i64 },
                    seg.state.as_str(),
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Fast batched progress checkpoint (called every ~500ms during active download).
    pub fn checkpoint_progress(
        &self,
        task_id: TaskId,
        downloaded_bytes: u64,
        total_size: Option<u64>,
        segments: &[SegmentInfo],
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("sqlite lock poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE tasks SET downloaded_bytes = ?1, total_size = COALESCE(?2, total_size) WHERE id = ?3",
            params![
                downloaded_bytes as i64,
                total_size.map(|v| v as i64),
                task_id.to_string(),
            ],
        )?;
        tx.execute(
            "DELETE FROM segments WHERE task_id = ?1",
            params![task_id.to_string()],
        )?;
        for seg in segments {
            tx.execute(
                "INSERT INTO segments (task_id, seg_index, start_offset, current_offset, end_offset, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    task_id.to_string(),
                    seg.index as i64,
                    seg.start_offset as i64,
                    seg.current_offset as i64,
                    if seg.end_offset == u64::MAX { -1i64 } else { seg.end_offset as i64 },
                    seg.state.as_str(),
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn append_log(&self, task_id: TaskId, entry: &TaskLogEntry) -> Result<()> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        conn.execute(
            "INSERT INTO history_events (task_id, timestamp, level, message) VALUES (?1, ?2, ?3, ?4)",
            params![
                task_id.to_string(),
                entry.timestamp.to_rfc3339(),
                entry.level,
                entry.message,
            ],
        )?;
        Ok(())
    }

    pub fn delete_task(&self, task_id: TaskId) -> Result<()> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        conn.execute("DELETE FROM tasks WHERE id = ?1", params![task_id.to_string()])?;
        Ok(())
    }

    pub fn load_all_tasks(&self) -> Result<Vec<DownloadTask>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT
                id, protocol, original_url, resolved_url, filename, save_dir,
                total_size, downloaded_bytes, status, supports_range,
                etag, last_modified, content_type, headers_json,
                max_connections, speed_limit_bps, category, matched_rule,
                checksum_json, source, error_message, created_at, completed_at
            FROM tasks
            ORDER BY created_at DESC
            "#,
        )?;

        let task_rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let protocol_str: String = row.get(1)?;
            let original_url: String = row.get(2)?;
            let resolved_url: String = row.get(3)?;
            let filename: String = row.get(4)?;
            let save_dir_str: String = row.get(5)?;
            let total_size: Option<i64> = row.get(6)?;
            let downloaded_bytes: i64 = row.get(7)?;
            let status_str: String = row.get(8)?;
            let supports_range: i64 = row.get(9)?;
            let etag: Option<String> = row.get(10)?;
            let last_modified: Option<String> = row.get(11)?;
            let content_type: Option<String> = row.get(12)?;
            let headers_json: String = row.get(13)?;
            let max_connections: i64 = row.get(14)?;
            let speed_limit_bps: i64 = row.get(15)?;
            let category_str: String = row.get(16)?;
            let matched_rule: Option<String> = row.get(17)?;
            let checksum_json: Option<String> = row.get(18)?;
            let source_str: String = row.get(19)?;
            let error_message: Option<String> = row.get(20)?;
            let created_at_str: String = row.get(21)?;
            let completed_at_str: Option<String> = row.get(22)?;

            let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
            let headers: BTreeMap<String, String> =
                serde_json::from_str(&headers_json).unwrap_or_default();
            let checksum: Option<ChecksumSpec> = checksum_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok());
            let source: TaskSource =
                serde_json::from_str(&source_str).unwrap_or(TaskSource::Manual);
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let completed_at = completed_at_str.as_deref().and_then(|s| {
                DateTime::parse_from_rfc3339(s)
                    .map(|d| d.with_timezone(&Utc))
                    .ok()
            });

            Ok(DownloadTask {
                id,
                protocol: ProtocolKind::from_str_lossy(&protocol_str),
                original_url,
                resolved_url,
                filename,
                save_dir: PathBuf::from(
                    save_dir_str.replace('/', &std::path::MAIN_SEPARATOR.to_string()),
                )
                .components()
                .collect(),
                total_size: total_size.map(|v| v.max(0) as u64),
                downloaded_bytes: downloaded_bytes.max(0) as u64,
                speed_bps: 0,
                eta_secs: None,
                status: TaskStatus::from_str_lossy(&status_str),
                supports_range: supports_range != 0,
                etag,
                last_modified,
                content_type,
                headers,
                max_connections: max_connections.clamp(1, 32) as u32,
                speed_limit_bps: speed_limit_bps.max(0) as u64,
                category: FileCategory::from_str_lossy(&category_str),
                matched_rule,
                checksum,
                source,
                error_message,
                created_at,
                completed_at,
                segments: Vec::new(),
                speed_history: Vec::new(),
                logs: Vec::new(),
            })
        })?;

        let mut tasks = Vec::new();
        for t in task_rows {
            tasks.push(t?);
        }

        let mut seg_stmt = conn.prepare(
            "SELECT seg_index, start_offset, current_offset, end_offset, state FROM segments WHERE task_id = ?1 ORDER BY seg_index ASC",
        )?;
        let mut log_stmt = conn.prepare(
            "SELECT timestamp, level, message FROM history_events WHERE task_id = ?1 ORDER BY id DESC LIMIT 40",
        )?;

        for task in &mut tasks {
            let tid = task.id.to_string();
            let segs = seg_stmt.query_map(params![&tid], |r| {
                let idx: i64 = r.get(0)?;
                let start: i64 = r.get(1)?;
                let cur: i64 = r.get(2)?;
                let end: i64 = r.get(3)?;
                let state_str: String = r.get(4)?;
                Ok(SegmentInfo {
                    index: idx.max(0) as u32,
                    start_offset: start.max(0) as u64,
                    current_offset: cur.max(0) as u64,
                    end_offset: if end < 0 { u64::MAX } else { end as u64 },
                    state: SegmentState::from_str_lossy(&state_str),
                    speed_bps: 0,
                })
            })?;
            for s in segs {
                task.segments.push(s?);
            }

            let mut logs: Vec<TaskLogEntry> = log_stmt
                .query_map(params![&tid], |r| {
                    let ts_str: String = r.get(0)?;
                    let level: String = r.get(1)?;
                    let message: String = r.get(2)?;
                    let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());
                    Ok(TaskLogEntry {
                        timestamp,
                        level,
                        message,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            logs.reverse();
            task.logs = logs;
        }

        Ok(tasks)
    }

    pub fn load_or_seed_rules(&self) -> Result<Vec<AutomationRule>> {
        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM rules", [], |r| r.get(0))
            .optional()?
            .unwrap_or(0);
        drop(conn);

        if count == 0 {
            let defaults = RuleEngine::default_rules();
            self.save_rules(&defaults)?;
            return Ok(defaults);
        }

        let conn = self.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare("SELECT rule_json FROM rules ORDER BY priority ASC")?;
        let mut rules: Vec<AutomationRule> = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .filter_map(|json| serde_json::from_str::<AutomationRule>(&json).ok())
            .collect();
        drop(stmt);
        drop(conn);

        let sep = std::path::MAIN_SEPARATOR.to_string();
        let mut migrated = false;
        for r in &mut rules {
            if let Some(tpl) = &mut r.action.save_dir_template
                && tpl.contains('/')
            {
                *tpl = tpl.replace('/', &sep);
                migrated = true;
            }
        }
        if migrated {
            let _ = self.save_rules(&rules);
        }
        Ok(rules)
    }

    pub fn save_rules(&self, rules: &[AutomationRule]) -> Result<()> {
        let mut conn = self.conn.lock().expect("sqlite lock poisoned");
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM rules", [])?;
        for rule in rules {
            let json = serde_json::to_string(rule)?;
            tx.execute(
                "INSERT INTO rules (id, priority, rule_json) VALUES (?1, ?2, ?3)",
                params![rule.id.to_string(), rule.priority, json],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_task_segments_and_crash_recovery() {
        let store = TaskStore::open_in_memory().unwrap();
        let mut task = DownloadTask {
            id: Uuid::new_v4(),
            protocol: ProtocolKind::Http,
            original_url: "https://example.com/a.iso".into(),
            resolved_url: "https://example.com/a.iso".into(),
            filename: "a.iso".into(),
            save_dir: PathBuf::from("C:\\Downloads"),
            total_size: Some(10_000),
            downloaded_bytes: 4_000,
            speed_bps: 0,
            eta_secs: None,
            status: TaskStatus::Downloading,
            supports_range: true,
            etag: Some("\"v1\"".into()),
            last_modified: None,
            content_type: Some("application/octet-stream".into()),
            headers: BTreeMap::new(),
            max_connections: 8,
            speed_limit_bps: 0,
            category: FileCategory::DiskImage,
            matched_rule: Some("系统镜像自动归档".into()),
            checksum: None,
            source: TaskSource::Manual,
            error_message: None,
            created_at: Utc::now(),
            completed_at: None,
            segments: vec![
                SegmentInfo {
                    index: 0,
                    start_offset: 0,
                    current_offset: 2_000,
                    end_offset: 4_999,
                    state: SegmentState::Active,
                    speed_bps: 1000,
                },
                SegmentInfo {
                    index: 1,
                    start_offset: 5_000,
                    current_offset: 7_000,
                    end_offset: 9_999,
                    state: SegmentState::Active,
                    speed_bps: 1000,
                },
            ],
            speed_history: vec![],
            logs: vec![],
        };

        store.upsert_task(&task).unwrap();
        let recovered = store.recover_interrupted_tasks().unwrap();
        assert_eq!(recovered, 1);

        let loaded = store.load_all_tasks().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].status, TaskStatus::Paused);
        assert_eq!(loaded[0].segments.len(), 2);
        assert_eq!(loaded[0].segments[0].state, SegmentState::Pending);

        task.status = TaskStatus::Completed;
        store.upsert_task(&task).unwrap();
        store.delete_task(task.id).unwrap();
        assert!(store.load_all_tasks().unwrap().is_empty());
    }
}
