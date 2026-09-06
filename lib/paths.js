import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';

export const VERSION = JSON.parse(fs.readFileSync(new URL('../package.json', import.meta.url), 'utf8')).version;
export const HOME = process.env.QORAL_HOME || path.join(os.homedir(), '.local', 'share', 'qoral');
export const DB_PATH = path.join(HOME, 'qoral.db');
export const AGENTS_DIR = path.join(HOME, 'agents');
export const SOCKET = process.env.QORAL_SOCKET || 'qoral';
export const NODE = process.execPath;
export const BIN = path.resolve(fileURLToPath(new URL('../bin/qoral.js', import.meta.url)));
export const HARNESSES = ['claude', 'codex', 'agy', 'gemini'];
export const HARNESS_LABELS = { claude: 'Claude Code', codex: 'OpenAI Codex', agy: 'Antigravity (agy)', gemini: 'Gemini CLI' };

export function ensureDirs() {
  fs.mkdirSync(AGENTS_DIR, { recursive: true });
}

export function agentDir(name) {
  const dir = path.join(AGENTS_DIR, name);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

export function expandHome(p) {
  if (!p) return p;
  if (p === '~') return os.homedir();
  if (p.startsWith('~/')) return path.join(os.homedir(), p.slice(2));
  return p;
}

/** Shell-quote a single argument (POSIX single quotes). */
export function q(s) {
  return `'${String(s).replace(/'/g, `'\\''`)}'`;
}
