import { DatabaseSync } from 'node:sqlite';
import { DB_PATH, ensureDirs } from './paths.js';

let db;

export function open() {
  if (db) return db;
  ensureDirs();
  db = new DatabaseSync(DB_PATH);
  db.exec(`
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
  `);
  return db;
}

const now = () => Date.now();

// ---- agents -------------------------------------------------------------

export function insertAgent({ name, harness, cwd, windowId, task }) {
  open()
    .prepare(`INSERT INTO agents(name, harness, cwd, window_id, status, task, created_at) VALUES (?, ?, ?, ?, 'starting', ?, ?)`)
    .run(name, harness, cwd, windowId ?? null, task ?? null, now());
}

export function getAgent(name) {
  return open().prepare(`SELECT * FROM agents WHERE name = ?`).get(name) ?? null;
}

export function listAgents({ activeOnly = false } = {}) {
  const where = activeOnly ? 'WHERE exited_at IS NULL' : '';
  return open().prepare(`SELECT * FROM agents ${where} ORDER BY created_at ASC`).all();
}

export function activeAgents() {
  return listAgents({ activeOnly: true });
}

export function setStatus(name, status) {
  open().prepare(`UPDATE agents SET status = ? WHERE name = ?`).run(status, name);
}

export function markExited(name) {
  open().prepare(`UPDATE agents SET status = 'exited', exited_at = COALESCE(exited_at, ?) WHERE name = ?`).run(now(), name);
}

export function deleteAgent(name) {
  open().prepare(`DELETE FROM agents WHERE name = ?`).run(name);
}

// ---- messages -----------------------------------------------------------

export function addMessage(sender, recipient, body) {
  const r = open()
    .prepare(`INSERT INTO messages(sender, recipient, body, ts) VALUES (?, ?, ?, ?)`)
    .run(sender, recipient, body, now());
  return Number(r.lastInsertRowid);
}

/** Messages for `recipient` that have not been fetched through the inbox tool. */
export function unreadFor(recipient) {
  return open().prepare(`SELECT * FROM messages WHERE recipient = ? AND read_at IS NULL ORDER BY id ASC`).all(recipient);
}

/** Messages that still need to be pushed into the agent's prompt. */
export function pendingNudges(recipient) {
  return open()
    .prepare(`SELECT * FROM messages WHERE recipient = ? AND read_at IS NULL AND nudged_at IS NULL ORDER BY id ASC`)
    .all(recipient);
}

/** Messages neither typed into the agent nor fetched by it yet. */
export function countPending(recipient) {
  return open()
    .prepare(`SELECT COUNT(*) AS n FROM messages WHERE recipient = ? AND read_at IS NULL AND nudged_at IS NULL`)
    .get(recipient).n;
}

export function countUnread(recipient) {
  return open().prepare(`SELECT COUNT(*) AS n FROM messages WHERE recipient = ? AND read_at IS NULL`).get(recipient).n;
}

export function markRead(ids) {
  if (!ids.length) return;
  const stmt = open().prepare(`UPDATE messages SET read_at = ? WHERE id = ?`);
  for (const id of ids) stmt.run(now(), id);
}

export function markNudged(ids) {
  if (!ids.length) return;
  const stmt = open().prepare(`UPDATE messages SET nudged_at = ? WHERE id = ?`);
  for (const id of ids) stmt.run(now(), id);
}

export function recentMessages(limit = 50) {
  return open()
    .prepare(`SELECT * FROM messages ORDER BY id DESC LIMIT ?`)
    .all(limit)
    .reverse();
}

export function messagesSince(id, limit = 200) {
  return open().prepare(`SELECT * FROM messages WHERE id > ? ORDER BY id ASC LIMIT ?`).all(id, limit);
}
