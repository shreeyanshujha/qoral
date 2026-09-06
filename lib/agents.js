import fs from 'node:fs';
import path from 'node:path';
import * as db from './db.js';
import * as tmux from './tmux.js';
import { buildLaunch } from './harness.js';
import { HARNESSES, NODE, BIN, agentDir, expandHome, q } from './paths.js';

const NAMES = ['ada', 'grace', 'linus', 'alan', 'dennis', 'ken', 'margaret', 'barbara', 'edsger', 'donald', 'radia', 'hedy', 'tim', 'guido', 'brendan', 'anders'];
const NAME_RE = /^[a-z0-9][a-z0-9_-]{0,31}$/;

export function suggestName() {
  const taken = new Set(db.listAgents().map((a) => a.name));
  return NAMES.find((n) => !taken.has(n)) ?? `agent-${Date.now().toString(36).slice(-4)}`;
}

export function spawnAgent({ harness, name, cwd, prompt, focus = true }) {
  if (!HARNESSES.includes(harness)) throw new Error(`unknown harness "${harness}" (claude | codex | gemini)`);
  name = (name || suggestName()).trim().toLowerCase();
  if (!NAME_RE.test(name)) throw new Error(`bad agent name "${name}" (letters, digits, - and _ only, max 32)`);
  if (['all', 'human', 'you', 'user', 'everyone'].includes(name)) throw new Error(`"${name}" is a reserved name`);

  cwd = path.resolve(expandHome(cwd || process.env.HOME));
  if (!fs.existsSync(cwd) || !fs.statSync(cwd).isDirectory()) throw new Error(`directory not found: ${cwd}`);

  const existing = db.getAgent(name);
  if (existing && !existing.exited_at && tmux.windowExists(existing.window_id)) {
    throw new Error(`an agent named "${name}" is already running`);
  }
  if (existing) db.deleteAgent(name);

  tmux.ensureAgentsSession();
  const spec = { ...buildLaunch({ harness, name, prompt }), harness, name, cwd, prompt: prompt ?? null };
  fs.writeFileSync(path.join(agentDir(name), 'launch.json'), JSON.stringify(spec, null, 2));

  const windowId = tmux.newWindow({ name, cwd, command: `${q(NODE)} ${q(BIN)} _run ${q(name)}` });
  db.insertAgent({ name, harness, cwd, windowId, task: prompt ? prompt.slice(0, 200) : null });
  if (focus) focusAgent(name);
  return db.getAgent(name);
}

/** Runs inside the agent's tmux window: exec the harness, then hold the window open on exit. */
export async function runAgent(name) {
  const specPath = path.join(agentDir(name), 'launch.json');
  const spec = JSON.parse(fs.readFileSync(specPath, 'utf8'));
  const { spawnSync } = await import('node:child_process');
  const r = spawnSync(spec.argv[0], spec.argv.slice(1), {
    stdio: 'inherit',
    cwd: spec.cwd,
    env: { ...process.env, ...spec.env, AGORA_AGENT: name },
  });
  db.markExited(name);
  const code = r.status ?? (r.error ? `error: ${r.error.message}` : 'signal');
  process.stdout.write(`\n\x1b[33m[agora] agent "${name}" exited\x1b[0m (${code}). Press Enter to close this window.\n`);
  await new Promise((resolve) => {
    process.stdin.resume();
    process.stdin.once('data', resolve);
  });
}

export function focusAgent(name) {
  const a = db.getAgent(name);
  if (!a || !a.window_id) throw new Error(`no agent named "${name}"`);
  tmux.selectWindow(a.window_id);
  tmux.focusAgentsPane();
}

/** focus by name, 1-based index, or next/prev relative to the active window. */
export function focusBy(target) {
  const agents = db.activeAgents().filter((a) => tmux.windowExists(a.window_id));
  if (!agents.length) return;
  if (target === 'next' || target === 'prev') {
    const cur = tmux.activeWindowId();
    let i = agents.findIndex((a) => a.window_id === cur);
    if (i < 0) i = target === 'next' ? -1 : 0;
    i = (i + (target === 'next' ? 1 : -1) + agents.length) % agents.length;
    return focusAgent(agents[i].name);
  }
  if (/^\d+$/.test(target)) {
    const a = agents[Number(target) - 1];
    if (a) return focusAgent(a.name);
    return;
  }
  return focusAgent(target);
}

export function killAgent(name) {
  const a = db.getAgent(name);
  if (!a) throw new Error(`no agent named "${name}"`);
  if (a.window_id) tmux.killWindow(a.window_id);
  db.deleteAgent(name);
}

export function resolveRecipient(to) {
  const t = String(to || '').trim().toLowerCase();
  if (!t) throw new Error('recipient is required');
  if (['human', 'you', 'user', 'operator'].includes(t)) return 'human';
  if (['all', 'everyone', '*'].includes(t)) return 'all';
  const a = db.getAgent(t);
  if (!a || a.exited_at) {
    const names = db.activeAgents().map((x) => x.name);
    throw new Error(`no active agent named "${t}". Active agents: ${names.join(', ') || '(none)'}; or use "human" / "all".`);
  }
  return t;
}

/** Send a message; returns list of recipients it was queued for. */
export function sendMessage(sender, to, body) {
  const recipient = resolveRecipient(to);
  body = String(body ?? '').trim();
  if (!body) throw new Error('message body is empty');
  if (recipient === 'all') {
    const targets = db.activeAgents().map((a) => a.name).filter((n) => n !== sender);
    for (const t of targets) db.addMessage(sender, t, body);
    return targets;
  }
  db.addMessage(sender, recipient, body);
  return [recipient];
}
