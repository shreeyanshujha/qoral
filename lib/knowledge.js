// Living documentation: per-project session notes + a rolling knowledge digest
// that is injected into every future agent's prompt for that project.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { HOME, HARNESSES } from './paths.js';

export const KNOWLEDGE_DIRNAME = '.agora';
const KNOWLEDGE_CAP = 8000; // chars injected into prompts
const TRANSCRIPT_HEAD = 8000;
const TRANSCRIPT_TAIL = 110000;
const MIN_TRANSCRIPT = 1500;

const NOTE_MARK = '===NOTE===';
const KNOW_MARK = '===KNOWLEDGE===';
const WRITER_SYSTEM =
  'You are a meticulous technical writer producing internal documentation for a software project from a terminal transcript. ' +
  'You have no tools and cannot read or write files; work only from the text you are given. ' +
  'Follow the formatting instructions exactly and output only the requested documents, with no preamble or commentary.';

export const knowledgeDir = (cwd) => path.join(cwd, KNOWLEDGE_DIRNAME);
export const knowledgeFile = (cwd) => path.join(knowledgeDir(cwd), 'KNOWLEDGE.md');
export const notesDir = (cwd) => path.join(knowledgeDir(cwd), 'notes');

const SKELETON = `# Project knowledge

## Overview

## Layout

## Conventions

## Decisions

## Gotchas

## How to run & test

## Notes from agents
`;

// ---- config -------------------------------------------------------------

export function loadConfig() {
  const defaults = { summarizer: 'auto', model: null, document_sessions: true };
  try {
    return { ...defaults, ...JSON.parse(fs.readFileSync(path.join(HOME, 'config.json'), 'utf8')) };
  } catch {
    return defaults;
  }
}

function which(cmd) {
  return spawnSync('sh', ['-c', `command -v ${cmd}`], { encoding: 'utf8' }).status === 0;
}

export function pickSummarizer(cfg = loadConfig()) {
  const want = process.env.AGORA_SUMMARIZER || cfg.summarizer || 'auto';
  if (want === 'none') return null;
  if (want !== 'auto') return HARNESSES.includes(want) && which(want) ? want : null;
  return HARNESSES.find(which) ?? null;
}

// ---- reading ------------------------------------------------------------

export function readKnowledge(cwd) {
  try {
    return fs.readFileSync(knowledgeFile(cwd), 'utf8');
  } catch {
    return '';
  }
}

export function listNotes(cwd, limit = 10) {
  let files;
  try {
    files = fs.readdirSync(notesDir(cwd)).filter((f) => f.endsWith('.md')).sort().reverse();
  } catch {
    return [];
  }
  return files.slice(0, limit).map((f) => {
    const p = path.join(notesDir(cwd), f);
    let title = f;
    try {
      const m = fs.readFileSync(p, 'utf8').match(/^#\s+(.+)$/m);
      if (m) title = m[1].trim();
    } catch {}
    return { file: path.join(KNOWLEDGE_DIRNAME, 'notes', f), title };
  });
}

/** Section appended to an agent's system prompt at launch. */
export function knowledgeSection(cwd) {
  const know = readKnowledge(cwd).trim();
  const notes = listNotes(cwd, 8);
  const lines = [];
  lines.push(`## Project knowledge (accumulated by earlier agora sessions in ${cwd})`);
  if (know) {
    lines.push(know.length > KNOWLEDGE_CAP ? know.slice(0, KNOWLEDGE_CAP) + '\n…(truncated; read .agora/KNOWLEDGE.md for the rest)' : know);
  } else {
    lines.push('(none yet — you are among the first agents in this project)');
  }
  if (notes.length) {
    lines.push('', 'Recent session notes (read them with your file tools when relevant):');
    for (const n of notes) lines.push(`- ${n.file} — ${n.title}`);
  }
  lines.push(
    '',
    'Record durable facts for future agents with agora_remember(text): architecture, conventions, decisions, gotchas, how to run/test. ' +
      'When your session ends it is summarized automatically into .agora/notes and merged into .agora/KNOWLEDGE.md.',
  );
  return lines.join('\n');
}

// ---- live additions -----------------------------------------------------

export function remember(cwd, agent, text) {
  text = String(text ?? '').trim();
  if (!text) throw new Error('nothing to remember');
  fs.mkdirSync(knowledgeDir(cwd), { recursive: true });
  let doc = readKnowledge(cwd) || SKELETON;
  const stamp = new Date().toISOString().slice(0, 10);
  const entry = `- (${agent}, ${stamp}) ${text.replace(/\s*\n\s*/g, ' ')}`;
  const heading = '## Notes from agents';
  if (!doc.includes(heading)) doc = doc.trimEnd() + `\n\n${heading}\n`;
  doc = doc.trimEnd() + '\n' + entry + '\n';
  fs.writeFileSync(knowledgeFile(cwd), doc);
  return knowledgeFile(cwd);
}

// ---- transcript handling ------------------------------------------------

const NOISE = /^[\s─━│┃╭╮╰╯├┤┬┴┼═║╔╗╚╝╠╣╦╩╬▀▄█▌▐░▒▓◐◓◑◒⏵·•✻✶✳✢*]+$/;

export function cleanTranscript(raw) {
  const lines = raw
    .replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, '')
    .split('\n')
    .map((l) => l.replace(/\s+$/, ''))
    .filter((l) => !NOISE.test(l));
  // collapse consecutive duplicate lines (spinner re-renders) and blank runs
  const out = [];
  for (const l of lines) {
    if (l === out[out.length - 1]) continue;
    if (l === '' && out[out.length - 1] === '') continue;
    out.push(l);
  }
  let text = out.join('\n').trim();
  if (text.length > TRANSCRIPT_HEAD + TRANSCRIPT_TAIL) {
    text = text.slice(0, TRANSCRIPT_HEAD) + '\n\n[… middle of transcript truncated …]\n\n' + text.slice(-TRANSCRIPT_TAIL);
  }
  return text;
}

// ---- summarizer ---------------------------------------------------------

function buildPrompt({ name, harness, cwd, task, startedAt, endedAt }) {
  const when = startedAt ? new Date(startedAt).toLocaleString() : 'unknown';
  const mins = startedAt && endedAt ? Math.round((endedAt - startedAt) / 60000) : null;
  return `You are writing documentation for a coding-agent session that just ended inside the "agora" multi-agent workspace.

Project directory: ${cwd}
Agent: ${name} (${harness}), started ${when}${mins !== null ? `, ran ${mins} min` : ''}
Task given at launch: ${task || '(none; interactive session)'}

After this prompt you will receive the CURRENT KNOWLEDGE document for the project (may be empty) and then the TERMINAL TRANSCRIPT of the session (it may contain rendering noise, tool output and other agents' messages prefixed "[agora]").

Produce exactly two markdown documents, separated by the marker lines shown, and nothing else.

${NOTE_MARK}
A session note. Use these sections; omit a section only if it would be empty:
# <one-line title: what the session accomplished>
## Summary
2–5 sentences.
## Changes
Files created / modified / deleted and what changed, as bullets with paths verbatim.
## Decisions
Choices made and the reasoning.
## Learnings
Gotchas, non-obvious facts about the codebase, commands that worked or failed.
## Open threads
Unfinished work, TODOs, questions for the next agent.
${KNOW_MARK}
The updated project knowledge document. Start from CURRENT KNOWLEDGE, merge in durable facts from this session (architecture, layout, conventions, decisions, gotchas, how to run and test), drop anything the session showed to be outdated, deduplicate, and keep the whole document under 6000 characters. It is injected into every future agent's system prompt for this project, so keep only what a new agent needs; no session-specific chatter. Use the headings: # Project knowledge, ## Overview, ## Layout, ## Conventions, ## Decisions, ## Gotchas, ## How to run & test, ## Notes from agents. Preserve the existing entries under "Notes from agents" (agents added them live) unless they are clearly obsolete.`;
}

export function runSummarizer(prompt, input, cwd, { summarizer, cfg } = {}) {
  cfg ??= loadConfig();
  summarizer ??= pickSummarizer(cfg);
  if (!summarizer) throw new Error('no summarizer available (set summarizer in ~/.local/share/agora/config.json)');
  const model = process.env.AGORA_SUMMARIZER_MODEL || cfg.model;
  const withModel = (flag) => (model ? [flag, model] : []);
  let argv, stdin;
  switch (summarizer) {
    case 'claude':
      // Replace Claude Code's own system prompt (memory instructions etc.), drop MCP servers and tools:
      // we want a plain writer, not an agent. stdin is appended to the prompt as context.
      argv = [
        'claude', '-p', '--output-format', 'text', '--tools', '', '--strict-mcp-config',
        '--system-prompt', WRITER_SYSTEM, ...withModel('--model'), prompt,
      ];
      stdin = input;
      break;
    case 'codex': // with no positional prompt, `exec` reads the entire prompt from stdin
      argv = ['codex', 'exec', '--skip-git-repo-check', '-C', cwd, '-s', 'read-only', ...withModel('-m')];
      stdin = `${prompt}\n\n${input}`;
      break;
    case 'gemini': // stdin is appended to -p prompt
      argv = ['gemini', '-p', prompt, ...withModel('-m')];
      stdin = input;
      break;
    default:
      throw new Error(`unknown summarizer ${summarizer}`);
  }
  const r = spawnSync(argv[0], argv.slice(1), {
    input: stdin,
    encoding: 'utf8',
    cwd,
    env: { ...process.env, AGORA_AGENT: undefined },
    maxBuffer: 64 * 1024 * 1024,
    timeout: 10 * 60 * 1000,
  });
  if (r.status !== 0) throw new Error(`${summarizer} exited ${r.status}: ${(r.stderr || '').trim().slice(-400)}`);
  return { summarizer, output: r.stdout };
}

const unfence = (s) => s.trim().replace(/^```(?:markdown|md)?\s*\n?/, '').replace(/\n?```\s*$/, '').trim();

function splitOutput(output) {
  const i = output.indexOf(NOTE_MARK);
  const j = output.indexOf(KNOW_MARK);
  if (i >= 0 && j > i) {
    return { note: unfence(output.slice(i + NOTE_MARK.length, j)), knowledge: unfence(output.slice(j + KNOW_MARK.length)) };
  }
  if (i >= 0) return { note: unfence(output.slice(i + NOTE_MARK.length)), knowledge: '' };
  // No markers: keep whatever came back as the note so nothing is lost, but don't touch KNOWLEDGE.md.
  const text = output.trim();
  return text ? { note: unfence(text), knowledge: '', degraded: true } : null;
}

function stamp(d = new Date()) {
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}`;
}

/**
 * Summarize a session transcript into .agora/notes/<stamp>-<agent>.md and merge into .agora/KNOWLEDGE.md.
 * Returns { skipped, reason } or { notePath, knowledgePath, summarizer }.
 */
export function documentSession({ name, harness, cwd, task, startedAt, endedAt, transcript, log = () => {} }) {
  const cfg = loadConfig();
  if (!cfg.document_sessions) return { skipped: true, reason: 'document_sessions=false in config' };
  const text = cleanTranscript(transcript || '');
  if (text.length < MIN_TRANSCRIPT) return { skipped: true, reason: 'session too short to document' };
  const summarizer = pickSummarizer(cfg);
  if (!summarizer) return { skipped: true, reason: 'no summarizer CLI available' };

  log(`summarizing ${Math.round(text.length / 1000)}k chars of transcript with ${summarizer}…`);
  const current = readKnowledge(cwd) || '(empty)';
  const prompt = buildPrompt({ name, harness, cwd, task, startedAt, endedAt: endedAt ?? Date.now() });
  const input = `CURRENT KNOWLEDGE:\n${current}\n\nTERMINAL TRANSCRIPT:\n${text}`;
  const { output } = runSummarizer(prompt, input, cwd, { summarizer, cfg });
  const parts = splitOutput(output);
  if (!parts || !parts.note) throw new Error(`summarizer returned nothing usable; raw output kept at ${saveRaw(cwd, name, output)}`);
  if (parts.degraded) log('summarizer ignored the format; saved its output as the note, knowledge left unchanged');

  fs.mkdirSync(notesDir(cwd), { recursive: true });
  const notePath = path.join(notesDir(cwd), `${stamp()}-${name}.md`);
  const header = `---\nagent: ${name}\nharness: ${harness}\ntask: ${JSON.stringify(task || '')}\nstarted: ${startedAt ? new Date(startedAt).toISOString() : ''}\nended: ${new Date(endedAt ?? Date.now()).toISOString()}\n---\n\n`;
  fs.writeFileSync(notePath, header + parts.note + '\n');

  let knowledgePath = null;
  if (parts.knowledge && parts.knowledge.length > 40) {
    knowledgePath = knowledgeFile(cwd);
    const prev = readKnowledge(cwd);
    if (prev) fs.writeFileSync(path.join(knowledgeDir(cwd), 'KNOWLEDGE.prev.md'), prev);
    fs.writeFileSync(knowledgePath, parts.knowledge + '\n');
  }
  return { notePath, knowledgePath, summarizer };
}

function saveRaw(cwd, name, output) {
  fs.mkdirSync(notesDir(cwd), { recursive: true });
  const p = path.join(notesDir(cwd), `${stamp()}-${name}.raw.txt`);
  fs.writeFileSync(p, output);
  return p;
}
