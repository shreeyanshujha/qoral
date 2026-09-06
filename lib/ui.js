// The sidebar TUI. Also hosts the message-delivery loop (bus) while it runs.
import * as db from './db.js';
import * as tmux from './tmux.js';
import { spawnAgent, focusAgent, killAgent, sendMessage, suggestName } from './agents.js';
import { createBus } from './bus.js';
import { HARNESSES, expandHome, BIN } from './paths.js';
import os from 'node:os';

const E = '\x1b[';
const S = {
  reset: `${E}0m`, bold: `${E}1m`, dim: `${E}2m`, inv: `${E}7m`, nobold: `${E}22m`,
  red: `${E}31m`, green: `${E}32m`, yellow: `${E}33m`, blue: `${E}34m`, magenta: `${E}35m`, cyan: `${E}36m`,
  gray: `${E}90m`, orange: `${E}38;5;214m`, white: `${E}97m`,
};
const ANSI_RE = /\x1b\[[0-9;]*m/g;
const vlen = (s) => s.replace(ANSI_RE, '').length;
const SPIN = ['◐', '◓', '◑', '◒'];
const HARNESS_TAG = {
  claude: `${S.orange}claude${S.reset}`,
  codex: `${S.green}codex${S.reset}`,
  agy: `${S.magenta}agy${S.reset}`,
  gemini: `${S.blue}gemini${S.reset}`,
  moderator: `${S.cyan}debate${S.reset}`,
};

function truncate(s, w) {
  // truncate visible text, keeping ANSI codes that precede the cut
  let out = '', n = 0, i = 0;
  while (i < s.length && n < w) {
    if (s[i] === '\x1b') {
      const m = s.slice(i).match(/^\x1b\[[0-9;]*m/);
      if (m) { out += m[0]; i += m[0].length; continue; }
    }
    out += s[i++]; n++;
  }
  if (i < s.length && n >= w) out = out.slice(0, out.length - 1) + '…';
  return out + S.reset;
}
function fit(s, w) {
  const l = vlen(s);
  return l > w ? truncate(s, w) : s + ' '.repeat(w - l);
}
function wrap(s, w) {
  const words = s.split(/\s+/);
  const lines = []; let cur = '';
  for (const wd of words) {
    if (!cur) { cur = wd; continue; }
    if (cur.length + 1 + wd.length <= w) cur += ' ' + wd; else { lines.push(cur); cur = wd; }
  }
  if (cur) lines.push(cur);
  return lines.length ? lines : [''];
}
const shortDir = (p) => p.replace(os.homedir(), '~');
const clock = (ts) => new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

export async function runUI() {
  db.open();
  const bus = createBus();
  const out = process.stdout;
  const state = {
    mode: 'list', // list | pick | input | confirm | log | help
    sel: 0,
    agents: [],
    frame: 0,
    flash: null, // { text, until }
    input: null, // { label, value, placeholder, onSubmit, onCancel }
    pick: null, // { label, options:[{key,label}], onPick }
    confirm: null, // { text, onYes }
    logScroll: 0,
    lastDir: os.homedir(),
    wizard: null,
  };

  const flash = (text, ms = 3000) => { state.flash = { text, until: Date.now() + ms }; };

  // ---------- rendering ----------
  function render() {
    const W = out.columns || 38, H = out.rows || 40;
    let lines = [];
    if (state.mode === 'log') lines = renderLog(W, H);
    else if (state.mode === 'help') lines = renderHelp(W, H);
    else lines = renderMain(W, H);
    while (lines.length < H) lines.push('');
    lines = lines.slice(0, H).map((l) => fit(l, W));
    out.write(`${E}?25l${E}H` + lines.join(`${E}K\n`) + `${E}K`);
    if (state.mode === 'input') out.write(`${E}?25h`);
  }

  function statusIcon(a) {
    switch (a.status) {
      case 'idle': return `${S.green}●${S.reset}`;
      case 'working': return `${S.yellow}${SPIN[state.frame % SPIN.length]}${S.reset}`;
      case 'attention': return `${S.red}${S.bold}!${S.reset}`;
      case 'documenting': return `${S.cyan}✎${S.reset}`;
      case 'moderating': return `${S.cyan}⚖${S.reset}`;
      case 'exited': return `${S.gray}○${S.reset}`;
      default: return `${S.gray}◌${S.reset}`;
    }
  }

  function renderMain(W, H) {
    const lines = [];
    const humanUnread = db.countUnread('human');
    const title = ` ${S.bold}${S.orange}agora${S.reset}`;
    const badge = humanUnread ? `${S.bold}${S.magenta}✉ ${humanUnread}${S.reset} ` : '';
    lines.push(fit(title, W - vlen(badge)) + badge);
    lines.push(`${S.gray}${'─'.repeat(W)}${S.reset}`);

    // header(2) + list + details(3) + bus(busH incl. its header) + footer(3: divider + 2 lines)
    const busH = Math.max(4, Math.min(10, Math.floor(H * 0.3)));
    const listH = Math.max(3, H - 2 - 3 - busH - 3);

    if (!state.agents.length) {
      lines.push(`${S.dim}  no agents yet${S.reset}`);
      lines.push(`${S.dim}  press ${S.reset}${S.bold}n${S.reset}${S.dim} to start one${S.reset}`);
    }
    const activeWin = tmux.activeWindowId();
    const start = Math.max(0, Math.min(state.sel - listH + 1, state.agents.length - listH));
    state.agents.slice(start, start + listH).forEach((a, i) => {
      const idx = start + i;
      const selected = idx === state.sel;
      const shown = a.window_id === activeWin;
      const unread = db.countPending(a.name);
      const cursor = selected ? `${S.orange}▸${S.reset}` : ' ';
      const num = idx < 9 ? `${S.gray}${idx + 1}${S.reset}` : ' ';
      const nameStyle = a.status === 'exited' ? S.gray : shown ? S.bold + S.white : '';
      const name = `${nameStyle}${a.name}${S.reset}`;
      const tag = a.status === 'exited' ? `${S.gray}exited${S.reset}` : HARNESS_TAG[a.harness] || a.harness;
      const mail = unread ? ` ${S.magenta}↓${unread}${S.reset}` : '';
      let row = `${cursor}${num} ${statusIcon(a)} ${fit(name, Math.max(6, W - 18))}${tag}${mail}`;
      if (selected) row = `${S.inv}${row.replace(ANSI_RE, '')}${S.reset}`;
      lines.push(row);
    });
    while (lines.length < 2 + Math.max(listH, 2)) lines.push('');

    // selected agent details
    const a = state.agents[state.sel];
    if (a) {
      lines.push(`${S.gray}${'─'.repeat(W)}${S.reset}`);
      lines.push(truncate(` ${S.dim}${shortDir(a.cwd)}${S.reset}`, W));
      lines.push(truncate(` ${S.dim}${a.task ? a.task.replace(/\s+/g, ' ') : '(no task given)'}${S.reset}`, W));
    } else {
      lines.push('', '', '');
    }

    // bus
    lines.push(`${S.gray}── bus ${'─'.repeat(Math.max(0, W - 7))}${S.reset}`);
    const msgs = db.recentMessages(busH - 1);
    const busLines = msgs.map((m) => {
      const toHuman = m.recipient === 'human';
      const col = toHuman ? S.magenta : m.sender === 'human' ? S.cyan : S.dim;
      const arrow = `${col}${m.sender}→${m.recipient}${S.reset}`;
      const unreadMark = toHuman && !m.read_at ? `${S.magenta}•${S.reset}` : ' ';
      return truncate(`${unreadMark}${arrow} ${m.body.replace(/\s+/g, ' ')}`, W);
    });
    if (!busLines.length) busLines.push(`${S.dim} quiet. agents talk via agora_send${S.reset}`);
    while (busLines.length < busH - 1) busLines.unshift('');
    lines.push(...busLines);

    // footer
    lines.push(`${S.gray}${'─'.repeat(W)}${S.reset}`);
    const k = (key, label) => `${S.bold}${key}${S.reset}${S.dim} ${label}${S.reset}`;
    if (state.flash && state.flash.until > Date.now()) {
      lines.push(truncate(` ${S.yellow}${state.flash.text}${S.reset}`, W));
      lines.push('');
    } else if (state.mode === 'input') {
      const inp = state.input;
      const shown = inp.value || `${S.dim}${inp.placeholder || ''}${S.reset}`;
      const label = ` ${S.cyan}${inp.label}${S.reset} `;
      const avail = W - vlen(label) - 1;
      let val = inp.value ? inp.value.slice(-avail) : shown;
      lines.push(label + val + (inp.value ? `${S.inv} ${S.reset}` : ''));
      lines.push(`${S.dim} Enter confirm · Esc cancel${inp.placeholder && !inp.value ? ' · empty = default' : ''}${S.reset}`);
    } else if (state.mode === 'pick') {
      lines.push(` ${S.cyan}${state.pick.label}${S.reset}`);
      lines.push(' ' + state.pick.options.map((o) => `${S.bold}${o.key}${S.reset}${S.dim} ${o.label}${S.reset}`).join('  '));
    } else if (state.mode === 'confirm') {
      lines.push(truncate(` ${S.red}${state.confirm.text}${S.reset}`, W));
      lines.push(`${S.dim} y confirm · any other key cancels${S.reset}`);
    } else {
      lines.push(` ${k('n', 'new')} ${k('D', 'debate')} ${k('m', 'msg')} ${k('b', 'bcast')} ${k('x', 'kill')}`);
      lines.push(` ${k('l', 'log')} ${k('o', 'docs')} ${k('?', 'help')} ${k('d', 'detach')} ${k('Q', 'quit')}`);
    }
    return lines;
  }

  function renderLog(W, H) {
    const msgs = db.recentMessages(300);
    db.markRead(msgs.filter((m) => m.recipient === 'human' && !m.read_at).map((m) => m.id));
    const rows = [];
    for (const m of msgs) {
      const col = m.recipient === 'human' ? S.magenta : m.sender === 'human' ? S.cyan : S.yellow;
      rows.push(`${S.dim}${clock(m.ts)}${S.reset} ${col}${S.bold}${m.sender}${S.nobold} → ${m.recipient}${S.reset}`);
      for (const l of wrap(m.body, W - 2)) rows.push(`  ${l}`);
      rows.push('');
    }
    if (!rows.length) rows.push(`${S.dim}  no messages yet${S.reset}`);
    const bodyH = H - 3;
    const maxScroll = Math.max(0, rows.length - bodyH);
    state.logScroll = Math.min(Math.max(0, state.logScroll), maxScroll);
    const from = maxScroll - state.logScroll; // scroll 0 = bottom
    const lines = [` ${S.bold}${S.orange}agora${S.reset} ${S.dim}message log${S.reset}`, `${S.gray}${'─'.repeat(W)}${S.reset}`];
    lines.push(...rows.slice(from, from + bodyH));
    while (lines.length < H - 1) lines.push('');
    lines.push(`${S.dim} j/k ↑/↓ scroll · g/G top/bottom · l/Esc back${S.reset}`);
    return lines;
  }

  function renderHelp(W) {
    const k = (key, label) => ` ${S.bold}${fit(key, 9)}${S.reset}${S.dim}${label}${S.reset}`;
    return [
      ` ${S.bold}${S.orange}agora${S.reset} ${S.dim}help${S.reset}`,
      `${S.gray}${'─'.repeat(W)}${S.reset}`,
      ` ${S.cyan}sidebar${S.reset}`,
      k('n', 'new agent: claude/codex/agy/gemini'),
      k('D', 'debate a question with 3 agents'),
      k('⏎ / →', 'show selected agent on the right'),
      k('j/k ↑/↓', 'move selection'),
      k('m', 'message selected agent'),
      k('b', 'broadcast to all agents'),
      k('x', 'kill selected agent'),
      k('l', 'message log'),
      k('o', "open project's .agora/KNOWLEDGE.md"),
      k('d', 'detach (agents keep running)'),
      k('Q', 'quit: kill every agent'),
      '',
      ` ${S.cyan}anywhere${S.reset}`,
      k('Alt+←/→', 'focus sidebar / agent'),
      k('Alt+[ ]', 'previous / next agent'),
      k('Alt+1..9', 'jump to agent N'),
      k('Alt+n', 'new agent'),
      k('Alt+m', 'message selected agent'),
      k('Alt+d', 'detach'),
      '',
      ` ${S.cyan}status${S.reset}`,
      ` ${S.green}●${S.reset}${S.dim} idle at prompt   ${S.reset}${S.yellow}◐${S.reset}${S.dim} working${S.reset}`,
      ` ${S.red}!${S.reset}${S.dim} needs you (permission/login)${S.reset}`,
      ` ${S.cyan}✎${S.reset}${S.dim} documenting session on exit${S.reset}`,
      ` ${S.gray}○${S.reset}${S.dim} exited (x removes it)${S.reset}`,
      '',
      ` ${S.cyan}agents talk via MCP tools${S.reset}`,
      `${S.dim} agora_send · agora_inbox · agora_wait${S.reset}`,
      `${S.dim} agora_spawn · agora_list_agents · agora_log${S.reset}`,
      `${S.dim} messages to an idle agent are typed into${S.reset}`,
      `${S.dim} its prompt automatically.${S.reset}`,
      '',
      `${S.dim} ? or Esc to go back${S.reset}`,
    ];
  }

  // ---------- actions ----------
  function reload() {
    state.agents = db.listAgents();
    state.sel = Math.min(state.sel, Math.max(0, state.agents.length - 1));
  }
  const selected = () => state.agents[state.sel];

  function askInput(label, { placeholder = '', initial = '' } = {}) {
    return new Promise((resolve) => {
      state.mode = 'input';
      state.input = { label, value: initial, placeholder, resolve };
      render();
    });
  }
  function askPick(label, options) {
    return new Promise((resolve) => {
      state.mode = 'pick';
      state.pick = { label, options, resolve };
      render();
    });
  }
  function askConfirm(text) {
    return new Promise((resolve) => {
      state.mode = 'confirm';
      state.confirm = { text, resolve };
      render();
    });
  }
  function backToList() {
    state.mode = 'list';
    state.input = state.pick = state.confirm = null;
    render();
  }

  async function newAgentWizard() {
    const harness = await askPick('harness?', [
      { key: 'c', label: 'claude', value: 'claude' },
      { key: 'x', label: 'codex', value: 'codex' },
      { key: 'a', label: 'agy', value: 'agy' },
      { key: 'g', label: 'gemini', value: 'gemini' },
    ]);
    if (!harness) return backToList();
    const defName = suggestName();
    const name = await askInput('name', { placeholder: defName });
    if (name === null) return backToList();
    const dir = await askInput('dir', { placeholder: shortDir(state.lastDir), initial: '' });
    if (dir === null) return backToList();
    const prompt = await askInput('task', { placeholder: '(optional)' });
    if (prompt === null) return backToList();
    backToList();
    try {
      const cwd = expandHome(dir.trim() || state.lastDir);
      const a = spawnAgent({ harness, name: name.trim() || defName, cwd, prompt: prompt.trim() || undefined, focus: true });
      state.lastDir = a.cwd;
      reload();
      state.sel = state.agents.findIndex((x) => x.name === a.name);
      flash(a.notices?.length ? `started ${a.name} · ${a.notices[0]}` : `started ${a.name} (${a.harness})`, a.notices?.length ? 9000 : 3000);
    } catch (e) {
      flash(`error: ${e.message}`, 6000);
    }
    render();
  }

  async function messageWizard(broadcast = false) {
    const a = selected();
    if (!broadcast && (!a || a.status === 'exited')) return flash('select a running agent first'), render();
    const to = broadcast ? 'all' : a.name;
    const body = await askInput(broadcast ? 'to all' : `to ${to}`, { placeholder: 'type a message' });
    backToList();
    if (body === null || !body.trim()) return;
    try {
      const targets = sendMessage('human', to, body.trim());
      flash(targets.length ? `sent to ${targets.join(', ')}` : 'no agents to send to');
    } catch (e) {
      flash(`error: ${e.message}`, 6000);
    }
    render();
  }

  async function killWizard() {
    const a = selected();
    if (!a) return;
    if (a.status === 'exited') {
      killAgent(a.name);
      reload();
      return render();
    }
    const yes = await askConfirm(`kill ${a.name}? (y/N)`);
    backToList();
    if (yes) {
      const { documenting } = killAgent(a.name);
      reload();
      flash(documenting ? `killed ${a.name} · documenting session in background` : `killed ${a.name}`, 5000);
    }
    render();
  }

  async function debateWizard() {
    const a = selected();
    const defDir = a && a.harness !== 'moderator' ? a.cwd : state.lastDir;
    const question = await askInput('debate', { placeholder: 'question to debate' });
    if (question === null || !question.trim()) return backToList();
    const dir = await askInput('dir', { placeholder: shortDir(defDir) });
    if (dir === null) return backToList();
    const build = await askPick('build the decision afterwards?', [
      { key: 'y', label: 'yes, spawn a builder', value: true },
      { key: 'n', label: 'no, decide only', value: false },
    ]);
    if (build === null) return backToList();
    backToList();
    const cwd = expandHome(dir.trim() || defDir);
    const cmd =
      `${JSON.stringify(process.execPath)} ${JSON.stringify(BIN)} debate ${JSON.stringify(question.trim())} --dir ${JSON.stringify(cwd)}${build ? ' --build' : ''}; ` +
      `echo; echo "[agora] debate finished. Press Enter to close."; read _`;
    try {
      const id = tmux.newWindow({ name: 'debate', cwd, command: cmd });
      tmux.selectWindow(id);
      tmux.focusAgentsPane();
      state.lastDir = cwd;
      flash('debate started · moderator output on the right');
    } catch (e) {
      flash(`error: ${e.message}`, 6000);
    }
    render();
  }

  function openKnowledge() {
    const a = selected();
    const cwd = a ? a.cwd : os.homedir();
    const file = `${cwd}/.agora/KNOWLEDGE.md`;
    const cmd = `sh -c 'if [ -f "$1" ]; then \${EDITOR:-less} "$1"; else echo "no knowledge yet at $1"; echo; echo "(sessions are documented when they end; agents can also call agora_remember)"; read _; fi' sh ${JSON.stringify(file)}`;
    try {
      const id = tmux.newWindow({ name: `knowledge`, cwd, command: cmd });
      tmux.selectWindow(id);
      tmux.focusAgentsPane();
    } catch (e) {
      flash(`error: ${e.message}`);
    }
  }

  async function quitAll() {
    const yes = await askConfirm('quit agora and kill every agent? (y/N)');
    backToList();
    if (yes) {
      for (const a of db.activeAgents()) db.markExited(a.name);
      tmux.killServer();
      process.exit(0);
    }
  }

  // ---------- input ----------
  function onKey(chunk) {
    const s = chunk.toString('utf8');
    // line-input mode
    if (state.mode === 'input') {
      const inp = state.input;
      if (s === '\x1b') { const r = inp.resolve; state.input = null; return r(null); }
      if (s === '\r' || s === '\n') { const r = inp.resolve; state.input = null; return r(inp.value); }
      if (s === '\x7f' || s === '\b') inp.value = inp.value.slice(0, -1);
      else if (s === '\x15') inp.value = '';
      else if (s === '\x17') inp.value = inp.value.replace(/\S+\s*$/, '');
      else if (s === '\x03') { const r = inp.resolve; state.input = null; return r(null); }
      else if (!s.startsWith('\x1b')) inp.value += s.replace(/[\r\n]+/g, ' ');
      return render();
    }
    if (state.mode === 'pick') {
      const opt = state.pick.options.find((o) => o.key === s);
      const r = state.pick.resolve;
      if (opt) { state.pick = null; return r(opt.value); }
      if (s === '\x1b' || s === 'q' || s === '\x03') { state.pick = null; return r(null); }
      return;
    }
    if (state.mode === 'confirm') {
      const r = state.confirm.resolve; state.confirm = null;
      return r(s === 'y' || s === 'Y');
    }
    if (state.mode === 'log') {
      if (s === 'j' || s === '\x1b[B') state.logScroll = Math.max(0, state.logScroll - 1);
      else if (s === 'k' || s === '\x1b[A') state.logScroll += 1;
      else if (s === 'G') state.logScroll = 0;
      else if (s === 'g') state.logScroll = 1e9;
      else if (s === '\x1b[6~') state.logScroll = Math.max(0, state.logScroll - 10);
      else if (s === '\x1b[5~') state.logScroll += 10;
      else if (s === 'l' || s === '\x1b' || s === 'q') state.mode = 'list';
      return render();
    }
    if (state.mode === 'help') {
      if (s === '?' || s === '\x1b' || s === 'q') state.mode = 'list';
      return render();
    }
    // list mode
    switch (s) {
      case 'j': case '\x1b[B': state.sel = Math.min(state.sel + 1, Math.max(0, state.agents.length - 1)); break;
      case 'k': case '\x1b[A': state.sel = Math.max(state.sel - 1, 0); break;
      case '\r': case '\n': case '\x1b[C': {
        const a = selected();
        if (a && a.window_id) { try { focusAgent(a.name); } catch (e) { flash(e.message); } }
        break;
      }
      case 'n': newAgentWizard(); return;
      case 'm': messageWizard(false); return;
      case 'b': messageWizard(true); return;
      case 'x': killWizard(); return;
      case 'l': state.mode = 'log'; state.logScroll = 0; break;
      case 'o': openKnowledge(); return;
      case 'D': debateWizard(); return;
      case '?': state.mode = 'help'; break;
      case 'd': tmux.detachAll(); break;
      case 'Q': quitAll(); return;
      case '\x03': flash('d detaches · Q quits all'); break;
      default:
        if (/^[1-9]$/.test(s)) {
          const a = state.agents[Number(s) - 1];
          if (a) { state.sel = Number(s) - 1; try { focusAgent(a.name); } catch {} }
        }
    }
    render();
  }

  // ---------- lifecycle ----------
  out.write(`${E}?1049h${E}?25l`);
  if (process.stdin.isTTY) process.stdin.setRawMode(true);
  process.stdin.resume();
  process.stdin.on('data', (c) => { try { onKey(c); } catch (e) { flash(`ui error: ${e.message}`, 5000); render(); } });
  out.on('resize', render);
  const cleanup = () => { out.write(`${E}?25h${E}?1049l`); };
  process.on('exit', cleanup);
  for (const sig of ['SIGTERM', 'SIGHUP']) process.on(sig, () => process.exit(0));

  reload();
  render();
  setInterval(() => {
    state.frame++;
    try { bus.tick(); } catch { /* transient tmux hiccup */ }
    reload();
    if (state.flash && state.flash.until < Date.now()) state.flash = null;
    render();
  }, 400);
  await new Promise(() => {});
}
