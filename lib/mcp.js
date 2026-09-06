// Minimal MCP server over stdio (newline-delimited JSON-RPC 2.0). Zero dependencies.
import * as db from './db.js';
import { spawnAgent, sendMessage } from './agents.js';
import { HARNESSES, VERSION } from './paths.js';
import { remember, readKnowledge, listNotes, loadConfig } from './knowledge.js';

const TOOLS = [
  {
    name: 'qoral_whoami',
    description: 'Your own qoral identity: name, harness, working directory.',
    inputSchema: { type: 'object', properties: {}, additionalProperties: false },
  },
  {
    name: 'qoral_list_agents',
    description: 'List every agent in the workspace with harness, working directory, current status (idle/working/attention/exited) and task.',
    inputSchema: { type: 'object', properties: {}, additionalProperties: false },
  },
  {
    name: 'qoral_send',
    description:
      'Send a message to another agent by name, to "all" (broadcast to every other agent) or to "human" (the operator). ' +
      'Recipients do not share your context: be self-contained and concise. Idle recipients receive it in their prompt within seconds; busy ones get it when they next become idle or call qoral_inbox.',
    inputSchema: {
      type: 'object',
      properties: {
        to: { type: 'string', description: 'Agent name, "all", or "human".' },
        message: { type: 'string', description: 'The message text.' },
      },
      required: ['to', 'message'],
      additionalProperties: false,
    },
  },
  {
    name: 'qoral_inbox',
    description: 'Fetch messages addressed to you that you have not read yet, oldest first. Marks them read.',
    inputSchema: { type: 'object', properties: {}, additionalProperties: false },
  },
  {
    name: 'qoral_wait',
    description:
      'Block until a message arrives for you (or the timeout passes), then return it. Use right after asking another agent a question. ' +
      'Returns immediately if unread messages already exist. If it times out, you may call it again.',
    inputSchema: {
      type: 'object',
      properties: {
        timeout_seconds: { type: 'number', description: 'Max seconds to wait (default 120, max 600).' },
        from: { type: 'string', description: 'Only wait for messages from this sender (optional).' },
      },
      additionalProperties: false,
    },
  },
  {
    name: 'qoral_spawn',
    description:
      'Start a new agent in the workspace and give it a task. The new agent knows your name and can message you back; ' +
      'follow up with qoral_wait to receive its report.',
    inputSchema: {
      type: 'object',
      properties: {
        harness: { type: 'string', enum: HARNESSES, description: 'Which CLI to run.' },
        name: { type: 'string', description: 'Short unique name (letters, digits, - _).' },
        cwd: { type: 'string', description: 'Working directory (defaults to yours).' },
        prompt: { type: 'string', description: 'The task to give the new agent.' },
      },
      required: ['harness', 'prompt'],
      additionalProperties: false,
    },
  },
  {
    name: 'qoral_log',
    description: 'Recent messages on the bus between all agents (for context). Default 30.',
    inputSchema: {
      type: 'object',
      properties: { limit: { type: 'number' } },
      additionalProperties: false,
    },
  },
  {
    name: 'qoral_remember',
    description:
      'Save a durable fact about this project for every future agent working here: architecture, conventions, a decision and its reasoning, a gotcha, how to run or test something. ' +
      'It is appended to .qoral/KNOWLEDGE.md immediately and injected into future agents\' prompts. One fact per call; be specific and concise.',
    inputSchema: {
      type: 'object',
      properties: { text: { type: 'string', description: 'The fact to remember.' } },
      required: ['text'],
      additionalProperties: false,
    },
  },
  {
    name: 'qoral_knowledge',
    description: 'Read the project\'s accumulated knowledge document (.qoral/KNOWLEDGE.md) and the list of recent session notes.',
    inputSchema: { type: 'object', properties: {}, additionalProperties: false },
  },
];

export const TOOL_NAMES = TOOLS.map((t) => t.name);

const text = (s) => ({ content: [{ type: 'text', text: s }] });
const errorResult = (s) => ({ content: [{ type: 'text', text: s }], isError: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function fmtMsg(m) {
  const when = new Date(m.ts).toLocaleTimeString();
  return `[${when}] ${m.sender} → ${m.recipient}: ${m.body}`;
}

export async function serveMcp(agentName) {
  if (!agentName) throw new Error('mcp: --agent <name> is required');
  db.open();
  const me = () => db.getAgent(agentName);

  async function callTool(name, args) {
    switch (name) {
      case 'qoral_whoami': {
        const a = me();
        return text(a ? `You are "${a.name}" (${a.harness}) in ${a.cwd}. Status: ${a.status}.` : `You are "${agentName}".`);
      }
      case 'qoral_list_agents': {
        const rows = db.listAgents();
        if (!rows.length) return text('No agents registered.');
        const lines = rows.map((a) => {
          const you = a.name === agentName ? ' (you)' : '';
          const task = a.task ? ` — task: ${a.task}` : '';
          return `- ${a.name}${you} [${a.harness}] ${a.status} in ${a.cwd}${task}`;
        });
        lines.push('- human [operator] reachable via qoral_send(to="human")');
        return text(lines.join('\n'));
      }
      case 'qoral_send': {
        const targets = sendMessage(agentName, args.to, args.message);
        if (!targets.length) return text('No other agents are running; nobody received the broadcast.');
        return text(`Queued for ${targets.join(', ')}. Use qoral_wait if you expect a reply.`);
      }
      case 'qoral_inbox': {
        const msgs = db.unreadFor(agentName);
        db.markRead(msgs.map((m) => m.id));
        if (!msgs.length) return text('Inbox empty.');
        return text(msgs.map(fmtMsg).join('\n'));
      }
      case 'qoral_wait': {
        const timeout = Math.min(Math.max(Number(args.timeout_seconds) || 120, 1), 600) * 1000;
        const from = args.from ? String(args.from).toLowerCase() : null;
        const deadline = Date.now() + timeout;
        while (true) {
          let msgs = db.unreadFor(agentName);
          if (from) msgs = msgs.filter((m) => m.sender === from);
          if (msgs.length) {
            db.markRead(msgs.map((m) => m.id));
            return text(msgs.map(fmtMsg).join('\n'));
          }
          if (Date.now() >= deadline) return text(`No message arrived within ${timeout / 1000}s. Call qoral_wait again or continue with other work.`);
          await sleep(400);
        }
      }
      case 'qoral_spawn': {
        // Cap agent-initiated spawning so a delegation loop can't fork-bomb the machine.
        const max = Number(loadConfig().max_agents) || 8;
        const running = db.activeAgents().filter((x) => x.harness !== 'moderator').length;
        if (running >= max) {
          return errorResult(`qoral: ${running} agents are already running, which is the max_agents limit (${max}). Reuse an existing agent via qoral_send, or ask the human to raise max_agents in ~/.local/share/qoral/config.json.`);
        }
        const a = spawnAgent({
          harness: args.harness,
          name: args.name,
          cwd: args.cwd || me()?.cwd,
          prompt: `${args.prompt}\n\n(You were started by agent "${agentName}". Report back to them with qoral_send(to="${agentName}") when done or if you have questions.)`,
          focus: false,
        });
        return text(`Started agent "${a.name}" (${a.harness}) in ${a.cwd}. It will report back to you; call qoral_wait(from="${a.name}") to receive it.`);
      }
      case 'qoral_log': {
        const msgs = db.recentMessages(Math.min(Number(args.limit) || 30, 200));
        return text(msgs.length ? msgs.map(fmtMsg).join('\n') : 'No messages yet.');
      }
      case 'qoral_remember': {
        const cwd = me()?.cwd || process.cwd();
        const file = remember(cwd, agentName, args.text);
        return text(`Saved to ${file}. Future agents in this project will see it.`);
      }
      case 'qoral_knowledge': {
        const cwd = me()?.cwd || process.cwd();
        const know = readKnowledge(cwd).trim();
        const notes = listNotes(cwd, 15);
        const out = [know || '(no knowledge recorded yet for this project)'];
        if (notes.length) out.push('', 'Session notes:', ...notes.map((n) => `- ${n.file} — ${n.title}`));
        return text(out.join('\n'));
      }
      default:
        throw Object.assign(new Error(`unknown tool ${name}`), { code: -32602 });
    }
  }

  async function dispatch(method, params) {
    switch (method) {
      case 'initialize':
        return {
          protocolVersion: params.protocolVersion || '2025-06-18',
          capabilities: { tools: {} },
          serverInfo: { name: 'qoral', version: VERSION },
          instructions: `You are agent "${agentName}" in an qoral workspace. Use qoral_send / qoral_wait to collaborate with the other agents.`,
        };
      case 'ping':
        return {};
      case 'tools/list':
        return { tools: TOOLS };
      case 'tools/call':
        try {
          return await callTool(params.name, params.arguments || {});
        } catch (e) {
          if (e.code === -32602) throw e;
          return errorResult(`qoral error: ${e.message}`);
        }
      case 'resources/list':
        return { resources: [] };
      case 'prompts/list':
        return { prompts: [] };
      default:
        throw Object.assign(new Error(`method not found: ${method}`), { code: -32601 });
    }
  }

  const write = (obj) => process.stdout.write(JSON.stringify(obj) + '\n');

  async function handle(msg) {
    if (msg.id === undefined || msg.id === null) return; // notification
    try {
      const result = await dispatch(msg.method, msg.params || {});
      write({ jsonrpc: '2.0', id: msg.id, result });
    } catch (e) {
      write({ jsonrpc: '2.0', id: msg.id, error: { code: e.code ?? -32000, message: e.message } });
    }
  }

  let buf = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (chunk) => {
    buf += chunk;
    let i;
    while ((i = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, i).trim();
      buf = buf.slice(i + 1);
      if (!line) continue;
      let msg;
      try {
        msg = JSON.parse(line);
      } catch {
        write({ jsonrpc: '2.0', id: null, error: { code: -32700, message: 'parse error' } });
        continue;
      }
      handle(msg);
    }
  });
  process.stdin.on('end', () => process.exit(0));
  await new Promise(() => {});
}
