//! SQLite message bus + agent registry. Schema-compatible with the Node implementation.
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::paths;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub struct Db {
    conn: Connection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRow {
    pub name: String,
    pub harness: String,
    pub cwd: String,
    pub window_id: Option<String>,
    pub status: String,
    pub task: Option<String>,
    pub created_at: i64,
    pub exited_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub sender: String,
    pub recipient: String,
    pub body: String,
    pub ts: i64,
    pub read_at: Option<i64>,
    pub nudged_at: Option<i64>,
}

impl Db {
    pub fn open() -> Result<Self> {
        paths::ensure_dirs()?;
        let conn = Connection::open(paths::db_path())?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA busy_timeout = 3000;
            CREATE TABLE IF NOT EXISTS agents (
              name       TEXT PRIMARY KEY,
              harness    TEXT NOT NULL,
              cwd        TEXT NOT NULL,
              window_id  TEXT,
              status     TEXT NOT NULL DEFAULT 'starting',
              task       TEXT,
              created_at INTEGER NOT NULL,
              exited_at  INTEGER
            );
            CREATE TABLE IF NOT EXISTS messages (
              id        INTEGER PRIMARY KEY AUTOINCREMENT,
              sender    TEXT NOT NULL,
              recipient TEXT NOT NULL,
              body      TEXT NOT NULL,
              ts        INTEGER NOT NULL,
              read_at   INTEGER,
              nudged_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_messages_recipient ON messages(recipient, read_at, nudged_at);
            "#,
        )?;
        Ok(Self { conn })
    }

    // ---- agents ----

    pub fn insert_agent(&self, name: &str, harness: &str, cwd: &str, window_id: Option<&str>, task: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO agents(name, harness, cwd, window_id, status, task, created_at) VALUES (?1, ?2, ?3, ?4, 'starting', ?5, ?6)",
            params![name, harness, cwd, window_id, task, now_ms()],
        )?;
        Ok(())
    }

    fn row_to_agent(r: &rusqlite::Row) -> rusqlite::Result<AgentRow> {
        Ok(AgentRow {
            name: r.get("name")?,
            harness: r.get("harness")?,
            cwd: r.get("cwd")?,
            window_id: r.get("window_id")?,
            status: r.get("status")?,
            task: r.get("task")?,
            created_at: r.get("created_at")?,
            exited_at: r.get("exited_at")?,
        })
    }

    pub fn get_agent(&self, name: &str) -> Result<Option<AgentRow>> {
        Ok(self
            .conn
            .query_row("SELECT * FROM agents WHERE name = ?1", params![name], Self::row_to_agent)
            .optional()?)
    }

    pub fn list_agents(&self, active_only: bool) -> Result<Vec<AgentRow>> {
        let sql = if active_only {
            "SELECT * FROM agents WHERE exited_at IS NULL ORDER BY created_at ASC"
        } else {
            "SELECT * FROM agents ORDER BY created_at ASC"
        };
        let mut st = self.conn.prepare(sql)?;
        let rows = st.query_map([], Self::row_to_agent)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn set_status(&self, name: &str, status: &str) -> Result<()> {
        self.conn.execute("UPDATE agents SET status = ?1 WHERE name = ?2", params![status, name])?;
        Ok(())
    }

    pub fn mark_exited(&self, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE agents SET status = 'exited', exited_at = COALESCE(exited_at, ?1) WHERE name = ?2",
            params![now_ms(), name],
        )?;
        Ok(())
    }

    pub fn delete_agent(&self, name: &str) -> Result<()> {
        self.conn.execute("DELETE FROM agents WHERE name = ?1", params![name])?;
        Ok(())
    }

    // ---- messages ----

    pub fn add_message(&self, sender: &str, recipient: &str, body: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO messages(sender, recipient, body, ts) VALUES (?1, ?2, ?3, ?4)",
            params![sender, recipient, body, now_ms()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn row_to_msg(r: &rusqlite::Row) -> rusqlite::Result<Message> {
        Ok(Message {
            id: r.get("id")?,
            sender: r.get("sender")?,
            recipient: r.get("recipient")?,
            body: r.get("body")?,
            ts: r.get("ts")?,
            read_at: r.get("read_at")?,
            nudged_at: r.get("nudged_at")?,
        })
    }

    pub fn unread_for(&self, recipient: &str) -> Result<Vec<Message>> {
        let mut st = self
            .conn
            .prepare("SELECT * FROM messages WHERE recipient = ?1 AND read_at IS NULL ORDER BY id ASC")?;
        let rows = st.query_map(params![recipient], Self::row_to_msg)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn pending_nudges(&self, recipient: &str) -> Result<Vec<Message>> {
        let mut st = self.conn.prepare(
            "SELECT * FROM messages WHERE recipient = ?1 AND read_at IS NULL AND nudged_at IS NULL ORDER BY id ASC",
        )?;
        let rows = st.query_map(params![recipient], Self::row_to_msg)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn count_pending(&self, recipient: &str) -> Result<u32> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE recipient = ?1 AND read_at IS NULL AND nudged_at IS NULL",
            params![recipient],
            |r| r.get::<_, i64>(0),
        )? as u32)
    }

    pub fn count_unread(&self, recipient: &str) -> Result<u32> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE recipient = ?1 AND read_at IS NULL",
            params![recipient],
            |r| r.get::<_, i64>(0),
        )? as u32)
    }

    pub fn mark_read(&self, ids: &[i64]) -> Result<()> {
        let now = now_ms();
        for id in ids {
            self.conn.execute("UPDATE messages SET read_at = ?1 WHERE id = ?2", params![now, id])?;
        }
        Ok(())
    }

    pub fn mark_nudged(&self, ids: &[i64]) -> Result<()> {
        let now = now_ms();
        for id in ids {
            self.conn.execute("UPDATE messages SET nudged_at = ?1 WHERE id = ?2", params![now, id])?;
        }
        Ok(())
    }

    pub fn recent_messages(&self, limit: usize) -> Result<Vec<Message>> {
        let mut st = self.conn.prepare("SELECT * FROM messages ORDER BY id DESC LIMIT ?1")?;
        let rows = st.query_map(params![limit as i64], Self::row_to_msg)?;
        let mut v: Vec<Message> = rows.filter_map(|r| r.ok()).collect();
        v.reverse();
        Ok(v)
    }

    #[allow(dead_code)] // used by the debate moderator once it is ported
    pub fn messages_since(&self, id: i64, limit: usize) -> Result<Vec<Message>> {
        let mut st = self.conn.prepare("SELECT * FROM messages WHERE id > ?1 ORDER BY id ASC LIMIT ?2")?;
        let rows = st.query_map(params![id, limit as i64], Self::row_to_msg)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}

/// Resolve a recipient the way the Node implementation does.
pub fn resolve_recipient(db: &Db, to: &str) -> Result<String> {
    let t = to.trim().to_lowercase();
    if t.is_empty() {
        anyhow::bail!("recipient is required");
    }
    if ["human", "you", "user", "operator"].contains(&t.as_str()) {
        return Ok("human".into());
    }
    if ["all", "everyone", "*"].contains(&t.as_str()) {
        return Ok("all".into());
    }
    match db.get_agent(&t)? {
        Some(a) if a.exited_at.is_none() => Ok(t),
        _ => {
            let names: Vec<String> = db.list_agents(true)?.into_iter().map(|a| a.name).collect();
            anyhow::bail!(
                "no active agent named \"{t}\". Active agents: {}; or use \"human\" / \"all\".",
                if names.is_empty() { "(none)".to_string() } else { names.join(", ") }
            )
        }
    }
}

/// Queue a message; returns the recipients it went to.
pub fn send_message(db: &Db, sender: &str, to: &str, body: &str) -> Result<Vec<String>> {
    let recipient = resolve_recipient(db, to)?;
    let body = body.trim();
    if body.is_empty() {
        anyhow::bail!("message body is empty");
    }
    if recipient == "all" {
        let targets: Vec<String> = db
            .list_agents(true)?
            .into_iter()
            .map(|a| a.name)
            .filter(|n| n != sender && n != "moderator")
            .collect();
        for t in &targets {
            db.add_message(sender, t, body)?;
        }
        return Ok(targets);
    }
    db.add_message(sender, &recipient, body)?;
    Ok(vec![recipient])
}
