import * as db from './db.js';
import * as tmux from './tmux.js';
import { spawnAgent, runAgent, focusBy, killAgent, sendMessage, snapshotAgent, documentSnapshot } from './agents.js';
import { HARNESSES, VERSION, BIN, expandHome } from './paths.js';
import { readKnowledge, listNotes, documentSession, pickSummarizer } from './knowledge.js';

const HELP = `agora ${VERSION} — many coding agents, one terminal, and they can talk.

usage:
  agora                          open the workspace (creates it if needed)
  agora up                       start it detached (for scripts)
  agora spawn <harness> [task]   start an agent   (--name x --dir path)
  agora send <to> <message>      message an agent ("all" broadcasts)   (--from name)
  agora list                     list agents and status
  agora log [-n 50]              recent bus traffic
  agora focus <name|N|next|prev> show an agent
  agora kill <name>              stop an agent (its session gets documented)
  agora stop                     kill the whole workspace
  agora debate "<question>"      3 agents argue it out, a decision doc is written
                                 (--dir path --agents claude,agy,codex --rounds 3
                                  --timeout 300 --build --keep)
  agora notes [dir]              show a project's knowledge + session notes
  agora document <name>          write a session note for a running agent now
  agora doctor                   check node, tmux, terminal and agent CLIs
  agora help

harnesses: ${HARNESSES.join(', ')}

living docs: when a session ends, its transcript is summarized into
  <project>/.agora/notes/<date>-<agent>.md and merged into
  <project>/.agora/KNOWLEDGE.md, which every future agent in that
  project receives in its prompt. Summarizer: ~/.local/share/agora/config.json
  {"summarizer": "auto|claude|codex|gemini|none", "model": null}
`;

const BOOLEAN_FLAGS = new Set(['no-focus', 'focus', 'help', 'no-docs', 'build', 'keep']);

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
    case 'up': { // start the workspace (sidebar + bus) detached, without attaching a terminal
      tmux.ensureUISession();
      console.log('workspace up (detached). run `agora` to attach.');
      return;
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
      for (const n of a.notices ?? []) console.error(`note: ${n}`);
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
      const { documenting } = killAgent(rest[0], { document: !flags['no-docs'] });
      console.log(`killed ${rest[0]}${documenting ? ' (documenting its session in the background)' : ''}`);
      return;
    }
    case 'debate': {
      const question = rest.join(' ').trim();
      if (!question) throw new Error('usage: agora debate "<question>" [--dir path] [--agents claude,agy,codex] [--rounds 3] [--timeout 300] [--build] [--keep]');
      const { runDebate } = await import('./debate.js');
      await runDebate({
        question,
        cwd: expandHome(flags.dir || process.cwd()),
        harnesses: flags.agents ? String(flags.agents).split(',').map((s) => s.trim()).filter(Boolean) : undefined,
        rounds: Number(flags.rounds) || 3,
        timeoutSec: Number(flags.timeout) || 300,
        build: !!flags.build,
        keep: !!flags.keep,
      });
      return;
    }
    case 'notes': case 'knowledge': case 'docs': {
      const cwd = expandHome(rest[0] || process.cwd());
      const know = readKnowledge(cwd).trim();
      console.log(know || `no knowledge recorded yet in ${cwd}/.agora/`);
      const notes = listNotes(cwd, Number(flags.n) || 20);
      if (notes.length) {
        console.log('\nsession notes:');
        for (const n of notes) console.log(`  ${n.file}  ${n.title}`);
      }
      return;
    }
    case 'document': case 'doc': {
      if (!rest[0]) throw new Error('usage: agora document <name>   (snapshot a running agent into a session note)');
      const snap = snapshotAgent(rest[0]);
      const res = documentSession({ ...snap, log: (m) => console.log(`[agora] ${m}`) });
      if (res.skipped) console.log(`not documented: ${res.reason}`);
      else console.log(`note: ${res.notePath}${res.knowledgePath ? `\nknowledge: ${res.knowledgePath}` : ''}`);
      return;
    }
    case 'summarizer':
      return console.log(pickSummarizer() ?? 'none');
    case 'doctor': {
      const { runDoctor } = await import('./doctor.js');
      process.exit(runDoctor() ? 1 : 0);
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
    case '_document': {
      const res = documentSnapshot(rest[0]);
      if (res.skipped) console.log(`not documented: ${res.reason}`);
      else console.log(`note: ${res.notePath}`);
      return;
    }
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
