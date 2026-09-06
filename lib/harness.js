import fs from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import os from 'node:os';
import { NODE, BIN, agentDir } from './paths.js';
import { knowledgeSection } from './knowledge.js';
import { TOOL_NAMES } from './mcp.js';

// ---- Antigravity CLI (agy) --------------------------------------------------
// agy has no per-session MCP flag: servers live in ~/.gemini/config/mcp_config.json and
// tool permissions in ~/.gemini/antigravity-cli/settings.json. Both are merged idempotently.
// The agent's identity reaches the MCP server through the AGORA_AGENT env var, which agy passes through.
const AGY_MCP_CONFIG = path.join(os.homedir(), '.gemini', 'config', 'mcp_config.json');
const AGY_SETTINGS = path.join(os.homedir(), '.gemini', 'antigravity-cli', 'settings.json');

function readJson(file, fallback) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    return fallback;
  }
}

/** Returns a list of human-readable notices describing any files it changed (empty if already set up). */
export function ensureAgyIntegration() {
  const notices = [];
  // 1. MCP server registration
  const mcp = readJson(AGY_MCP_CONFIG, {});
  mcp.mcpServers ??= {};
  const want = { command: NODE, args: [BIN, 'mcp'], disabled: false };
  const have = mcp.mcpServers.agora;
  if (!have || have.command !== want.command || JSON.stringify(have.args) !== JSON.stringify(want.args) || have.disabled) {
    mcp.mcpServers.agora = { ...(have || {}), ...want };
    fs.mkdirSync(path.dirname(AGY_MCP_CONFIG), { recursive: true });
    fs.writeFileSync(AGY_MCP_CONFIG, JSON.stringify(mcp, null, 2) + '\n');
    notices.push(`registered the agora MCP server for Antigravity in ${AGY_MCP_CONFIG} (remove with: agy mcp remove agora)`);
  }
  // 2. Pre-approve agora tools so the bus never blocks on "Do you want to proceed?"
  const settings = readJson(AGY_SETTINGS, {});
  settings.permissions ??= {};
  settings.permissions.allow ??= [];
  const rules = TOOL_NAMES.map((t) => `mcp(agora/${t})`);
  const missing = rules.filter((r) => !settings.permissions.allow.includes(r));
  if (missing.length) {
    settings.permissions.allow.push(...missing);
    fs.mkdirSync(path.dirname(AGY_SETTINGS), { recursive: true });
    fs.writeFileSync(AGY_SETTINGS, JSON.stringify(settings, null, 2) + '\n');
    notices.push(`added ${missing.length} mcp(agora/*) allow rules to ${AGY_SETTINGS} so agents aren't prompted for bus calls`);
  }
  return notices;
}

export function intro(name, harness) {
  return [
    `You are running inside agora, a shared terminal workspace where several AI coding agents work side by side and can talk to each other.`,
    `Your agent name is "${name}" (harness: ${harness}). Other agents, and the human operator (addressed as "human"), are reachable through the \`agora\` MCP tools:`,
    `- agora_list_agents: who else is running, what they are working on, and whether they are idle.`,
    `- agora_send(to, message): send a message to another agent by name, to "all" to broadcast, or to "human" to reach the person.`,
    `- agora_inbox(): fetch messages addressed to you that you have not read yet.`,
    `- agora_wait(timeout_seconds): block until a message arrives. Use it right after asking another agent something so you get the answer.`,
    `- agora_spawn(harness, name, cwd, prompt): start a new agent and delegate work to it.`,
    `- agora_log(limit): recent traffic on the bus, for context.`,
    `- agora_remember(text): save a durable fact about this project for every future agent (goes into .agora/KNOWLEDGE.md).`,
    `- agora_knowledge(): read the project's accumulated knowledge and the list of past session notes.`,
    `Messages may also be delivered straight into your prompt as a line starting with "[agora] message from X:". Treat those exactly like a request from X: act on it and reply with agora_send(to="X") rather than answering in plain text, because X cannot see your screen.`,
    `Recipients do not share your context, so make every message self-contained: what you need, why, and the relevant file paths or snippets. Keep it concise.`,
    `Coordinate before editing files another agent is likely touching. When you finish a piece of delegated work, report back to whoever asked. If you have no task yet, say hello briefly and wait for instructions.`,
  ].join('\n');
}

/**
 * Build the launch spec for an agent. Returns { argv, env } and writes any per-agent config files.
 */
export function buildLaunch({ harness, name, prompt, cwd }) {
  const dir = agentDir(name);
  const mcpArgs = [BIN, 'mcp', '--agent', name];
  const mcpJson = { mcpServers: { agora: { command: NODE, args: mcpArgs } } };
  const system = `${intro(name, harness)}\n\n${knowledgeSection(cwd)}`;
  const fullPrompt = prompt ? `${system}\n\nYour task:\n${prompt}` : system;

  switch (harness) {
    case 'claude': {
      const cfg = path.join(dir, 'mcp.json');
      fs.writeFileSync(cfg, JSON.stringify(mcpJson, null, 2));
      // mcp__agora pre-approves every tool from the agora server so the bus never blocks on a permission prompt.
      // A pinned session id lets us find Claude's own JSONL transcript when documenting the session.
      const sessionId = randomUUID();
      const argv = [
        'claude', '--name', name, '--session-id', sessionId, '--mcp-config', cfg,
        '--allowedTools', 'mcp__agora', '--append-system-prompt', system,
      ];
      if (prompt) argv.push(prompt);
      return { argv, env: {}, sessionId };
    }
    case 'codex': {
      // Codex takes config overrides as TOML key=value pairs. JSON string/array literals are valid TOML.
      const argv = [
        'codex',
        '-c',
        `mcp_servers.agora.command=${JSON.stringify(NODE)}`,
        '-c',
        `mcp_servers.agora.args=${JSON.stringify(mcpArgs)}`,
        fullPrompt,
      ];
      return { argv, env: {} };
    }
    case 'agy': {
      const notices = ensureAgyIntegration();
      // No system-prompt flag: the intro rides in the initial interactive prompt.
      return { argv: ['agy', '-i', fullPrompt], env: {}, notices };
    }
    case 'gemini': {
      const cfg = path.join(dir, 'gemini-settings.json');
      // trust:true skips Gemini's per-tool confirmation for the agora server.
      const geminiJson = { mcpServers: { agora: { ...mcpJson.mcpServers.agora, trust: true } } };
      fs.writeFileSync(cfg, JSON.stringify(geminiJson, null, 2));
      const argv = ['gemini', '-i', fullPrompt];
      return { argv, env: { GEMINI_CLI_SYSTEM_SETTINGS_PATH: cfg } };
    }
    default:
      throw new Error(`unknown harness "${harness}" (expected claude, codex, agy or gemini)`);
  }
}
