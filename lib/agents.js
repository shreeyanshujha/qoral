import fs from 'node:fs';
import path from 'node:path';
import * as db from './db.js';
import * as tmux from './tmux.js';
import { buildLaunch } from './harness.js';
import { HARNESSES, NODE, BIN, agentDir, expandHome, q } from './paths.js';
import { documentSession } from './knowledge.js';
import { collectTranscript } from './transcript.js';
import { spawn as spawnProcess } from 'node:child_process';

const NAMES = ['ada', 'grace', 'linus', 'alan', 'dennis', 'ken', 'margaret', 'barbara', 'edsger', 'donald', 'radia', 'hedy', 'tim', 'guido', 'brendan', 'anders'];
const NAME_RE = /^[a-z0-9][a-z0-9_-]{0,31}$/;

export function suggestName() {
  const taken = new Set(db.listAgents().map((a) => a.name));
  return NAMES.find((n) => !taken.has(n)) ?? `agent-${Date.now().toString(36).slice(-4)}`;
}

export function spawnAgent({ harness, name, cwd, prompt, focus = true }) {
  if (!HARNESSES.includes(harness)) throw new Error(`unknown harness "${harness}" (${HARNESSES.join(' | ')})`);
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
  const dir = agentDir(name);
  const rawLog = path.join(dir, 'raw.log');
  try { fs.unlinkSync(rawLog); } catch {}
  const spec = {
    ...buildLaunch({ harness, name, prompt, cwd }),
    harness, name, cwd, prompt: prompt ?? null, startedAt: Date.now(), rawLog,
  };
  fs.writeFileSync(path.join(dir, 'launch.json'), JSON.stringify(spec, null, 2));

  const windowId = tmux.newWindow({ name, cwd, command: `${q(NODE)} ${q(BIN)} _run ${q(name)}` });
  tmux.pipePane(windowId, rawLog); // raw output log: fallback transcript for documentation
  db.insertAgent({ name, harness, cwd, windowId, task: prompt ? prompt.slice(0, 200) : null });
  if (focus) focusAgent(name);
  return { ...db.getAgent(name), notices: spec.notices ?? [] };
}

/** Runs inside the agent's tmux window: exec the harness, then hold the window open on exit. */
export async function runAgent(name) {
  const specPath = path.join(agentDir(name), 'launch.json');
  const spec = JSON.parse(fs.readFileSync(specPath, 'utf8'));
  const { spawnSync } = await import('node:child_process');
  const r = spawnSync(spec.argv[0], spec.argv.slice(1), {
    stdio: 'inherit',
    cwd: spec.cwd,
    env: { ...process.env, ...spec.env, QORAL_AGENT: name },
  });
  const code = r.status ?? (r.error ? `error: ${r.error.message}` : 'signal');

  // Living documentation: summarize what happened before the window goes away.
  const Y = '\x1b[33m', D = '\x1b[2m', R = '\x1b[0m';
  db.setStatus(name, 'documenting');
  process.stdout.write(`\n${Y}[qoral] documenting session…${R} ${D}(writes .qoral/notes and .qoral/KNOWLEDGE.md in ${spec.cwd})${R}\n`);
  try {
    const log = (m) => process.stdout.write(`${D}[qoral] ${m}${R}\n`);
    const got = collectTranscript({ ...spec, paneTarget: process.env.TMUX_PANE });
    if (got) log(`transcript source: ${got.source}`);
    const res = documentSession({
      name, harness: spec.harness, cwd: spec.cwd, task: spec.prompt, startedAt: spec.startedAt, endedAt: Date.now(),
      transcript: got?.text ?? '', log,
    });
    if (res.skipped) process.stdout.write(`${D}[qoral] not documented: ${res.reason}${R}\n`);
    else process.stdout.write(`${D}[qoral] note: ${res.notePath}${res.knowledgePath ? `\n[qoral] knowledge: ${res.knowledgePath}` : ''}${R}\n`);
  } catch (e) {
    process.stdout.write(`${Y}[qoral] documentation failed:${R} ${e.message}\n`);
  }

  db.markExited(name);
  process.stdout.write(`\n${Y}[qoral] agent "${name}" exited${R} (${code}). Press Enter to close this window.\n`);
  await new Promise((resolve) => {
    process.stdin.resume();
    process.stdin.once('data', resolve);
  });
}

/** Snapshot an agent's transcript now (used before killing it, or for a mid-session checkpoint). */
export function snapshotAgent(name) {
  const a = db.getAgent(name);
  if (!a) throw new Error(`no agent named "${name}"`);
  const dir = agentDir(name);
  let spec = {};
  try { spec = JSON.parse(fs.readFileSync(path.join(dir, 'launch.json'), 'utf8')); } catch {}
  const paneTarget = a.window_id && tmux.windowExists(a.window_id) ? a.window_id : null;
  const got = collectTranscript({ ...spec, harness: a.harness, cwd: a.cwd, paneTarget });
  const session = {
    name, harness: a.harness, cwd: a.cwd, task: a.task,
    startedAt: spec.startedAt ?? a.created_at, endedAt: Date.now(), source: got?.source ?? 'none',
  };
  fs.writeFileSync(path.join(dir, 'transcript.txt'), got?.text ?? '');
  fs.writeFileSync(path.join(dir, 'session.json'), JSON.stringify(session, null, 2));
  return { ...session, transcript: got?.text ?? '' };
}

/** Document a previously snapshotted session (runs in a detached process after a kill). */
export function documentSnapshot(name, log = () => {}) {
  const dir = agentDir(name);
  const session = JSON.parse(fs.readFileSync(path.join(dir, 'session.json'), 'utf8'));
  const transcript = fs.readFileSync(path.join(dir, 'transcript.txt'), 'utf8');
  return documentSession({ ...session, transcript, log });
}

function documentInBackground(name) {
  const child = spawnProcess(NODE, [BIN, '_document', name], { detached: true, stdio: 'ignore', env: process.env });
  child.unref();
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

export function killAgent(name, { document = true } = {}) {
  const a = db.getAgent(name);
  if (!a) throw new Error(`no agent named "${name}"`);
  let documenting = false;
  if (document && !a.exited_at && a.window_id && tmux.windowExists(a.window_id)) {
    try {
      snapshotAgent(name);
      documentInBackground(name);
      documenting = true;
    } catch {}
  }
  if (a.window_id) tmux.killWindow(a.window_id);
  db.deleteAgent(name);
  return { documenting };
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
