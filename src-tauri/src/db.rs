//! Local SQLite store shared by the app and the Canvas MCP helper process.
//! Holds the write-approval queue, scheduled jobs, notification de-duplication
//! keys and an audit log. Secrets are never stored here.
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

pub struct Db {
    conn: Connection,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub const JOB_KINDS: &[&str] = &["deadline_alert", "new_announcements", "grade_updates", "ai_briefing"];

impl Db {
    pub fn open(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("데이터 폴더를 만들 수 없습니다: {e}"))?;
        let conn = Connection::open(dir.join("canvas-assistant.db")).map_err(|e| format!("데이터베이스를 열 수 없습니다: {e}"))?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS pending_actions(
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               created_at INTEGER NOT NULL,
               kind TEXT NOT NULL,
               summary TEXT NOT NULL,
               reason TEXT,
               payload TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'pending',
               result TEXT,
               decided_at INTEGER);
             CREATE TABLE IF NOT EXISTS jobs(
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               kind TEXT NOT NULL UNIQUE,
               enabled INTEGER NOT NULL DEFAULT 0,
               interval_min INTEGER NOT NULL,
               params TEXT NOT NULL DEFAULT '{}',
               last_run INTEGER,
               last_result TEXT);
             CREATE TABLE IF NOT EXISTS notified(
               key TEXT PRIMARY KEY,
               at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS audit_log(
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               at INTEGER NOT NULL,
               action TEXT NOT NULL,
               detail TEXT NOT NULL,
               ok INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS briefings(
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               at INTEGER NOT NULL,
               text TEXT NOT NULL);",
        ).map_err(|e| format!("데이터베이스 초기화 실패: {e}"))?;
        let db = Self { conn };
        db.seed_jobs()?;
        Ok(db)
    }

    fn seed_jobs(&self) -> Result<(), String> {
        let defaults = [
            ("deadline_alert", 1, 30, json!({"hours_before": [24, 3]})),
            ("new_announcements", 1, 30, json!({})),
            ("grade_updates", 1, 60, json!({})),
            ("ai_briefing", 0, 1440, json!({"time": "08:00", "prompt": "이번 주 마감 과제와 최근 공지를 확인해서 오늘 해야 할 일을 우선순위대로 짧게 정리해줘."})),
        ];
        for (kind, enabled, interval, params) in defaults {
            self.conn.execute(
                "INSERT OR IGNORE INTO jobs(kind, enabled, interval_min, params) VALUES (?1, ?2, ?3, ?4)",
                params![kind, enabled, interval, params.to_string()],
            ).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    // ----- approval queue -----

    pub fn add_pending(&self, kind: &str, summary: &str, reason: &str, payload: &Value) -> Result<i64, String> {
        self.conn.execute(
            "INSERT INTO pending_actions(created_at, kind, summary, reason, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now(), kind, summary, reason, payload.to_string()],
        ).map_err(|e| e.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_pending(&self, include_done: bool) -> Result<Vec<Value>, String> {
        let sql = if include_done {
            "SELECT id, created_at, kind, summary, reason, payload, status, result, decided_at FROM pending_actions ORDER BY id DESC LIMIT 100"
        } else {
            "SELECT id, created_at, kind, summary, reason, payload, status, result, decided_at FROM pending_actions WHERE status='pending' ORDER BY id DESC"
        };
        let mut stmt = self.conn.prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?, "createdAt": r.get::<_, i64>(1)?, "kind": r.get::<_, String>(2)?,
                "summary": r.get::<_, String>(3)?, "reason": r.get::<_, Option<String>>(4)?,
                "payload": serde_json::from_str::<Value>(&r.get::<_, String>(5)?).unwrap_or(Value::Null),
                "status": r.get::<_, String>(6)?, "result": r.get::<_, Option<String>>(7)?, "decidedAt": r.get::<_, Option<i64>>(8)?,
            }))
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn pending_count(&self) -> i64 {
        self.conn.query_row("SELECT COUNT(*) FROM pending_actions WHERE status='pending'", [], |r| r.get(0)).unwrap_or(0)
    }

    /// Atomically claims a pending action so it can only be executed once.
    pub fn claim_pending(&self, id: i64) -> Result<(String, Value), String> {
        let changed = self.conn.execute(
            "UPDATE pending_actions SET status='running' WHERE id=?1 AND status='pending'", params![id],
        ).map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err("이미 처리되었거나 없는 요청입니다.".into());
        }
        self.conn.query_row("SELECT kind, payload FROM pending_actions WHERE id=?1", params![id], |r| {
            Ok((r.get::<_, String>(0)?, serde_json::from_str::<Value>(&r.get::<_, String>(1)?).unwrap_or(Value::Null)))
        }).map_err(|e| e.to_string())
    }

    pub fn finish_pending(&self, id: i64, status: &str, result: &str) -> Result<(), String> {
        self.conn.execute(
            "UPDATE pending_actions SET status=?2, result=?3, decided_at=?4 WHERE id=?1",
            params![id, status, result, now()],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn reject_pending(&self, id: i64) -> Result<(), String> {
        let changed = self.conn.execute(
            "UPDATE pending_actions SET status='rejected', decided_at=?2 WHERE id=?1 AND status='pending'", params![id, now()],
        ).map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err("이미 처리되었거나 없는 요청입니다.".into());
        }
        Ok(())
    }

    /// Requests left 'running' by a crash are returned to the queue as failed.
    pub fn recover_stuck(&self) {
        let _ = self.conn.execute("UPDATE pending_actions SET status='failed', result='앱이 종료되어 실행 결과를 확인하지 못했습니다. Canvas에서 직접 확인하세요.' WHERE status='running'", []);
    }

    // ----- audit log -----

    pub fn audit(&self, action: &str, detail: &str, ok: bool) {
        let _ = self.conn.execute(
            "INSERT INTO audit_log(at, action, detail, ok) VALUES (?1, ?2, ?3, ?4)",
            params![now(), action, detail, ok as i64],
        );
        let _ = self.conn.execute("DELETE FROM audit_log WHERE id NOT IN (SELECT id FROM audit_log ORDER BY id DESC LIMIT 500)", []);
    }

    pub fn audit_list(&self) -> Result<Vec<Value>, String> {
        let mut stmt = self.conn.prepare("SELECT at, action, detail, ok FROM audit_log ORDER BY id DESC LIMIT 100").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| Ok(json!({"at": r.get::<_, i64>(0)?, "action": r.get::<_, String>(1)?, "detail": r.get::<_, String>(2)?, "ok": r.get::<_, i64>(3)? == 1})))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    // ----- jobs -----

    pub fn jobs(&self) -> Result<Vec<Value>, String> {
        let mut stmt = self.conn.prepare("SELECT id, kind, enabled, interval_min, params, last_run, last_result FROM jobs ORDER BY id").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| Ok(json!({
            "id": r.get::<_, i64>(0)?, "kind": r.get::<_, String>(1)?, "enabled": r.get::<_, i64>(2)? == 1,
            "intervalMin": r.get::<_, i64>(3)?, "params": serde_json::from_str::<Value>(&r.get::<_, String>(4)?).unwrap_or(json!({})),
            "lastRun": r.get::<_, Option<i64>>(5)?, "lastResult": r.get::<_, Option<String>>(6)?,
        }))).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn update_job(&self, kind: &str, enabled: bool, interval_min: i64, params: &Value) -> Result<(), String> {
        if !JOB_KINDS.contains(&kind) {
            return Err("알 수 없는 작업입니다".into());
        }
        let changed = self.conn.execute(
            "UPDATE jobs SET enabled=?2, interval_min=?3, params=?4 WHERE kind=?1",
            params![kind, enabled as i64, interval_min.clamp(10, 10080), params.to_string()],
        ).map_err(|e| e.to_string())?;
        if changed == 0 { Err("작업을 찾을 수 없습니다".into()) } else { Ok(()) }
    }

    pub fn mark_job_run(&self, kind: &str, result: &str) {
        let _ = self.conn.execute("UPDATE jobs SET last_run=?2, last_result=?3 WHERE kind=?1", params![kind, now(), result]);
    }

    pub fn force_job_due(&self, kind: &str) {
        let _ = self.conn.execute("UPDATE jobs SET last_run=NULL WHERE kind=?1", params![kind]);
    }

    // ----- notification de-duplication -----

    /// Returns true the first time a key is seen.
    pub fn first_time(&self, key: &str) -> bool {
        self.conn.execute("INSERT OR IGNORE INTO notified(key, at) VALUES (?1, ?2)", params![key, now()]).map(|n| n == 1).unwrap_or(false)
    }

    pub fn seen(&self, key: &str) -> bool {
        self.conn.query_row("SELECT 1 FROM notified WHERE key=?1", params![key], |_| Ok(())).optional().ok().flatten().is_some()
    }

    pub fn prune_notified(&self, older_than_days: i64) {
        let _ = self.conn.execute("DELETE FROM notified WHERE at < ?1 AND key NOT LIKE 'seeded:%' AND key <> 'tray-hint'", params![now() - older_than_days * 86400]);
    }

    // ----- AI briefings -----

    pub fn add_briefing(&self, text: &str) {
        let _ = self.conn.execute("INSERT INTO briefings(at, text) VALUES (?1, ?2)", params![now(), text]);
        let _ = self.conn.execute("DELETE FROM briefings WHERE id NOT IN (SELECT id FROM briefings ORDER BY id DESC LIMIT 30)", []);
    }

    pub fn briefings(&self) -> Result<Vec<Value>, String> {
        let mut stmt = self.conn.prepare("SELECT id, at, text FROM briefings ORDER BY id DESC LIMIT 30").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_, i64>(0)?, "at": r.get::<_, i64>(1)?, "text": r.get::<_, String>(2)?})))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> (tempdir::Guard, Db) {
        let g = tempdir::Guard::new();
        let db = Db::open(&g.0).unwrap();
        (g, db)
    }

    mod tempdir {
        pub struct Guard(pub std::path::PathBuf);
        impl Guard {
            pub fn new() -> Self {
                let p = std::env::temp_dir().join(format!("ca-test-{}-{}", std::process::id(), rand_suffix()));
                std::fs::create_dir_all(&p).unwrap();
                Self(p)
            }
        }
        impl Drop for Guard {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        fn rand_suffix() -> u128 {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        }
    }

    #[test]
    fn pending_actions_execute_once() {
        let (_g, db) = temp();
        let id = db.add_pending("planner_note", "할 일", "테스트", &json!({"title": "x"})).unwrap();
        assert_eq!(db.pending_count(), 1);
        let (kind, payload) = db.claim_pending(id).unwrap();
        assert_eq!(kind, "planner_note");
        assert_eq!(payload["title"], "x");
        assert!(db.claim_pending(id).is_err(), "second claim must fail");
        db.finish_pending(id, "done", "ok").unwrap();
        assert_eq!(db.pending_count(), 0);
        assert!(db.reject_pending(id).is_err());
    }

    #[test]
    fn jobs_are_seeded_and_validated() {
        let (_g, db) = temp();
        let jobs = db.jobs().unwrap();
        assert_eq!(jobs.len(), JOB_KINDS.len());
        db.update_job("ai_briefing", true, 5, &json!({"time": "09:00"})).unwrap();
        let j = db.jobs().unwrap().into_iter().find(|j| j["kind"] == "ai_briefing").unwrap();
        assert_eq!(j["enabled"], true);
        assert_eq!(j["intervalMin"], 10, "interval is clamped");
        assert!(db.update_job("rm -rf", true, 60, &json!({})).is_err());
    }

    #[test]
    fn notification_keys_dedupe() {
        let (_g, db) = temp();
        assert!(db.first_time("a:1"));
        assert!(!db.first_time("a:1"));
        assert!(db.seen("a:1"));
        assert!(!db.seen("a:2"));
    }
}
