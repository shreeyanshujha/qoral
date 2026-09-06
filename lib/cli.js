import * as db from './db.js';
import * as tmux from './tmux.js';
import { spawnAgent, runAgent, focusBy, killAgent, sendMessage } from './agents.js';
import { HARNESSES, VERSION, BIN } from './paths.js';

const HELP = `agora ${VERSION} — many coding agents, one terminal, and they can talk.

usage:
  agora                          open the workspace (creates it if needed)
  agora spawn <harness> [task]   start an agent   (--name x --dir path)
  agora send <to> <message>      message an agent ("all" broadcasts)   (--from name)
  agora list                     list agents and status
  agora log [-n 50]              recent bus traffic
  agora focus <name|N|next|prev> show an agent
  agora kill <name>              stop an agent
  agora stop                     kill the whole workspace
  agora help

harnesses: ${HARNESSES.join(', ')}
`;

const BOOLEAN_FLAGS = new Set(['no-focus', 'focus', 'help']);

function parseFlags(argv) {
  const flags = {}, rest = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith('--') && a.length > 2) {
      const [k, v] = a.slice(2).split(/=(.*)/s);
      if (v !== undefined) flags[k] = v;
      else if (BOOLEAN_FLAGS.has(k)) flags[k] = true;
      else if (i + 1 < argv.length && !argv[i + 1].startsWith('--')) flags[k] = argv[++i];
      else flags[k] = true;
    } else if (a === '-n' && i + 1 < argv.length) flags.n = argv[++i];
    else rest.push(a);
  }
  return { flags, rest };
}

export async function main(argv) {
  const [cmd = 'open', ...args] = argv;
  const { flags, rest } = parseFlags(args);

  switch (cmd) {
    case 'open': case 'start': case 'attach': {
      tmux.ensureUISession();
      process.exit(tmux.attach(tmux.UI));
    }
    case 'spawn': case 'new': case 'run': {
      const [harness, ...promptParts] = rest;
      if (!harness) throw new Error('usage: agora spawn <claude|codex|gemini> [task] [--name x] [--dir path]');
      const a = spawnAgent({
        harness,
        name: flags.name,
        cwd: flags.dir || flags.cwd || process.cwd(),
        prompt: promptParts.join(' ') || flags.prompt || undefined,
        focus: !flags['no-focus'],
      });
      console.log(`started ${a.name} (${a.harness}) in ${a.cwd}`);
      if (!tmux.hasSession(tmux.UI)) console.log(`run \`agora\` to open the workspace`);
      return;
    }
    case 'send': case 'msg': case 'tell': {
      const [to, ...body] = rest;
      if (!to || !body.length) throw new Error('usage: agora send <agent|all|human> <message>');
      const targets = sendMessage(flags.from || 'human', to, body.join(' '));
      console.log(targets.length ? `queued for ${targets.join(', ')}` : 'no recipients');
      return;
    }
    case 'list': case 'ls': case 'ps': {
      const rows = db.listAgents();
      if (!rows.length) return console.log('no agents');
      for (const a of rows) {
        const unread = db.countPending(a.name);
        console.log(`${a.name.padEnd(14)} ${a.harness.padEnd(7)} ${a.status.padEnd(10)} ${unread ? `↓${unread} ` : ''}${a.cwd}${a.task ? `  — ${a.task}` : ''}`);
      }
      const h = db.countUnread('human');
      if (h) console.log(`\n${h} unread message(s) for you — see \`agora log\``);
      return;
    }
    case 'log': {
      const n = Number(flags.n) || 50;
      const msgs = db.recentMessages(n);
      db.markRead(msgs.filter((m) => m.recipient === 'human' && !m.read_at).map((m) => m.id));
      for (const m of msgs) console.log(`${new Date(m.ts).toLocaleTimeString()} ${m.sender} → ${m.recipient}: ${m.body}`);
      if (!msgs.length) console.log('no messages yet');
      return;
    }
    case 'focus': case 'show': {
      if (!rest[0]) throw new Error('usage: agora focus <name|N|next|prev>');
      focusBy(rest[0]);
      return;
    }
    case 'kill': case 'rm': {
      if (!rest[0]) throw new Error('usage: agora kill <name>');
      killAgent(rest[0]);
      console.log(`killed ${rest[0]}`);
      return;
    }
    case 'stop': case 'quit': {
      for (const a of db.activeAgents()) db.markExited(a.name);
      tmux.killServer();
      console.log('agora stopped');
      return;
    }
    case 'mcp': {
      const { serveMcp } = await import('./mcp.js');
      return serveMcp(flags.agent || process.env.AGORA_AGENT);
    }
    // ---- internal ----
    case 'ui': {
      const { runUI } = await import('./ui.js');
      return runUI();
    }
    case 'home': {
      const { runHome } = await import('./home.js');
      return runHome();
    }
    case '_run':
      return runAgent(rest[0]);
    case '_attach-inner': {
      tmux.ensureAgentsSession();
      process.exit(tmux.attach(tmux.AGENTS));
    }
    case 'where':
      return console.log(BIN);
    case 'help': case '--help': case '-h':
      return process.stdout.write(HELP);
    case 'version': case '--version': case '-v':
      return console.log(VERSION);
    default:
      throw new Error(`unknown command "${cmd}"\n\n${HELP}`);
  }
}
