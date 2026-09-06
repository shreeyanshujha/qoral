// Collect the best available transcript of a finished (or running) agent session.
// Sources, in order of preference:
//   1. the harness's own session file (Claude JSONL, Codex rollout JSONL, Gemini chat JSON)
//   2. the raw pane output log agora taps with `tmux pipe-pane` from launch
//   3. whatever tmux scrollback is left (Claude Code clears it, others may not)
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import * as tmux from './tmux.js';
import { cleanTranscript } from './knowledge.js';

const CLIP = 800;
const clip = (s, n = CLIP) => {
  s = typeof s === 'string' ? s : JSON.stringify(s);
  return s.length > n ? s.slice(0, n) + ` …[+${s.length - n} chars]` : s;
};

function readJsonl(file) {
  const out = [];
  for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
    if (!line.trim()) continue;
    try {
      out.push(JSON.parse(line));
    } catch {}
  }
  return out;
}

function newestFile(candidates, since) {
  let best = null;
  for (const f of candidates) {
    try {
      const st = fs.statSync(f);
      if (since && st.mtimeMs < since - 60_000) continue;
      if (!best || st.mtimeMs > best.mtime) best = { file: f, mtime: st.mtimeMs };
    } catch {}
  }
  return best?.file ?? null;
}

function globDir(dir, pred) {
  try {
    return fs.readdirSync(dir).filter(pred).map((f) => path.join(dir, f));
  } catch {
    return [];
  }
}

// ---- Claude Code ---------------------------------------------------------

function renderContent(content, roleLabel) {
  const lines = [];
  if (typeof content === 'string') {
    if (content.trim()) lines.push(`${roleLabel}: ${content.trim()}`);
    return lines;
  }
  for (const part of content || []) {
    switch (part.type) {
      case 'text':
        if (part.text?.trim()) lines.push(`${roleLabel}: ${part.text.trim()}`);
        break;
      case 'tool_use':
        lines.push(`TOOL CALL ${part.name}(${clip(part.input, 600)})`);
        break;
      case 'tool_result': {
        const body = Array.isArray(part.content)
          ? part.content.map((c) => (c.type === 'text' ? c.text : `[${c.type}]`)).join('\n')
          : part.content ?? '';
        lines.push(`TOOL RESULT: ${clip(body)}`);
        break;
      }
      default:
        break; // thinking, images, etc.
    }
  }
  return lines;
}

export function claudeTranscript({ sessionId, cwd, startedAt }) {
  const root = path.join(os.homedir(), '.claude', 'projects');
  let file = null;
  if (sessionId) {
    for (const dir of globDir(root, () => true)) {
      const f = path.join(dir, `${sessionId}.jsonl`);
      if (fs.existsSync(f)) {
        file = f;
        break;
      }
    }
  }
  if (!file && cwd) {
    // fall back: newest transcript in the project's slug directory, modified since launch
    const slug = cwd.replace(/[^A-Za-z0-9]/g, '-');
    file = newestFile(globDir(path.join(root, slug), (f) => f.endsWith('.jsonl')), startedAt);
  }
  if (!file) return null;
  const lines = [];
  for (const rec of readJsonl(file)) {
    if (rec.isMeta) continue;
    if (rec.type === 'user' && rec.message) lines.push(...renderContent(rec.message.content, 'USER'));
    else if (rec.type === 'assistant' && rec.message) lines.push(...renderContent(rec.message.content, 'ASSISTANT'));
  }
  return lines.length ? { source: `claude transcript ${path.basename(file)}`, text: lines.join('\n') } : null;
}

// ---- Codex ----------------------------------------------------------------

function walkFiles(dir, pred, depth = 4, acc = []) {
  if (depth < 0) return acc;
  for (const entry of globDir(dir, () => true)) {
    try {
      const st = fs.statSync(entry);
      if (st.isDirectory()) walkFiles(entry, pred, depth - 1, acc);
      else if (pred(entry)) acc.push(entry);
    } catch {}
  }
  return acc;
}

export function codexTranscript({ cwd, startedAt }) {
  const root = path.join(os.homedir(), '.codex', 'sessions');
  const files = walkFiles(root, (f) => f.endsWith('.jsonl')).filter((f) => {
    try {
      return fs.statSync(f).mtimeMs >= (startedAt ?? 0) - 60_000;
    } catch {
      return false;
    }
  });
  // prefer a rollout whose session_meta cwd matches ours
  let file = null;
  for (const f of files.sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs)) {
    const head = fs.readFileSync(f, 'utf8').slice(0, 4000);
    if (!cwd || head.includes(JSON.stringify(cwd))) {
      file = f;
      break;
    }
  }
  if (!file) return null;
  const lines = [];
  for (const rec of readJsonl(file)) {
    const p = rec.payload ?? rec;
    const t = p.type ?? rec.type;
    if (t === 'message' && p.role) {
      const text = (Array.isArray(p.content) ? p.content : [p.content])
        .map((c) => (typeof c === 'string' ? c : c?.text ?? ''))
        .join('\n')
        .trim();
      if (text) lines.push(`${p.role.toUpperCase()}: ${text}`);
    } else if (t === 'function_call' || t === 'local_shell_call' || t === 'custom_tool_call') {
      lines.push(`TOOL CALL ${p.name ?? t}(${clip(p.arguments ?? p.action ?? p.input ?? '', 600)})`);
    } else if (t === 'function_call_output' || t === 'custom_tool_call_output') {
      lines.push(`TOOL RESULT: ${clip(p.output ?? '')}`);
    } else if (t === 'agent_message' && p.message) {
      lines.push(`ASSISTANT: ${p.message}`);
    } else if (t === 'user_message' && p.message) {
      lines.push(`USER: ${p.message}`);
    }
  }
  return lines.length ? { source: `codex rollout ${path.basename(file)}`, text: lines.join('\n') } : null;
}

// ---- Gemini CLI -----------------------------------------------------------

export function geminiTranscript({ startedAt }) {
  const root = path.join(os.homedir(), '.gemini', 'tmp');
  const files = walkFiles(root, (f) => /chats[\\/].*\.json$/.test(f) || /session-.*\.json$/.test(f), 3);
  const file = newestFile(files, startedAt);
  if (!file) return null;
  let data;
  try {
    data = JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    return null;
  }
  const msgs = data.messages ?? data.history ?? [];
  const lines = [];
  for (const m of msgs) {
    const role = (m.type ?? m.role ?? '').toString().toUpperCase().replace('GEMINI', 'ASSISTANT').replace('MODEL', 'ASSISTANT');
    const content = Array.isArray(m.content) ? m.content.map((c) => c.text ?? '').join('\n') : m.content ?? m.text ?? '';
    if (content?.trim()) lines.push(`${role || 'MESSAGE'}: ${content.trim()}`);
    for (const tc of m.toolCalls ?? []) {
      lines.push(`TOOL CALL ${tc.name}(${clip(tc.args ?? {}, 600)})`);
      if (tc.result) lines.push(`TOOL RESULT: ${clip(tc.result)}`);
    }
  }
  return lines.length ? { source: `gemini chat ${path.basename(file)}`, text: lines.join('\n') } : null;
}

// ---- raw pane log + scrollback -------------------------------------------

export function rawLogTranscript(rawLog) {
  if (!rawLog || !fs.existsSync(rawLog)) return null;
  const size = fs.statSync(rawLog).size;
  if (!size) return null;
  const MAX = 4 * 1024 * 1024;
  const fd = fs.openSync(rawLog, 'r');
  const buf = Buffer.alloc(Math.min(size, MAX));
  fs.readSync(fd, buf, 0, buf.length, Math.max(0, size - buf.length));
  fs.closeSync(fd);
  // turn cursor movement into line breaks so redraw fragments don't run together
  const text = buf
    .toString('utf8')
    .replace(/\x1b\[[0-9;]*[ABCDEFGHJKSTf]/g, '\n')
    .replace(/\r/g, '\n');
  return { source: 'raw pane log', text };
}

export function scrollbackTranscript(target) {
  const text = target ? tmux.captureAll(target) : '';
  return text.trim() ? { source: 'tmux scrollback', text } : null;
}

// ---- entry point ----------------------------------------------------------

/**
 * Returns { text, source } with the richest transcript available, or null.
 * `minChars` filters sources that are too thin to be worth summarizing.
 */
export function collectTranscript({ harness, cwd, sessionId, startedAt, rawLog, paneTarget }, minChars = 1500) {
  const attempts = [];
  const tryAdd = (fn) => {
    try {
      const r = fn();
      if (r) attempts.push(r);
    } catch {}
  };
  if (harness === 'claude') tryAdd(() => claudeTranscript({ sessionId, cwd, startedAt }));
  if (harness === 'codex') tryAdd(() => codexTranscript({ cwd, startedAt }));
  if (harness === 'gemini') tryAdd(() => geminiTranscript({ startedAt }));
  tryAdd(() => rawLogTranscript(rawLog));
  tryAdd(() => scrollbackTranscript(paneTarget));
  for (const a of attempts) {
    if (cleanTranscript(a.text).length >= minChars) return a;
  }
  // nothing rich enough; return the longest so the caller can report why it skipped
  return attempts.sort((a, b) => b.text.length - a.text.length)[0] ?? null;
}
