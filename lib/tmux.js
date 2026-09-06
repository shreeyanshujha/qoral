import { execFileSync, spawnSync } from 'node:child_process';
import { SOCKET, NODE, BIN, q } from './paths.js';

export const AGENTS = 'agents'; // inner session: one window per agent
export const UI = 'agora'; // outer session: sidebar | nested attach to `agents`

export function tmux(...args) {
  return execFileSync('tmux', ['-L', SOCKET, ...args], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).replace(/\n$/, '');
}

export function tmuxTry(...args) {
  try {
    return tmux(...args);
  } catch {
    return null;
  }
}

export function hasSession(name) {
  return tmuxTry('has-session', '-t', `=${name}`) !== null;
}

export function serverUp() {
  return hasSession(AGENTS);
}

const self = `${q(NODE)} ${q(BIN)}`;

/** Create the detached `agents` session plus server-wide options and key bindings. Idempotent. */
export function ensureAgentsSession() {
  if (hasSession(AGENTS)) return;
  tmux('new-session', '-d', '-s', AGENTS, '-n', '_home', '-x', '200', '-y', '50', `${self} home`);

  // server / global options
  tmux('set', '-s', 'escape-time', '10');
  tmux('set', '-s', 'focus-events', 'on');
  tmux('set', '-g', 'mouse', 'on');
  tmux('set', '-g', 'history-limit', '50000');
  tmux('set', '-g', 'default-terminal', 'tmux-256color');
  tmux('set', '-ga', 'terminal-overrides', ',*:Tc');
  tmux('set', '-g', 'set-clipboard', 'on');
  tmux('set', '-g', 'allow-passthrough', 'on');

  // inner session: invisible chrome, no prefix, agents own the whole pane
  tmux('set', '-t', AGENTS, 'status', 'off');
  tmux('set', '-t', AGENTS, 'prefix', 'None');
  tmux('set', '-t', AGENTS, 'prefix2', 'None');

  // global Alt bindings (server-wide root table). The outer client sees them first,
  // so the nested client / agents never receive these particular chords.
  const bind = (key, ...cmd) => tmux('bind', '-n', key, ...cmd);
  bind('M-Left', 'select-pane', '-t', '{top-left}');
  bind('M-Right', 'select-pane', '-t', '{bottom-right}');
  bind('M-h', 'select-pane', '-t', '{top-left}');
  bind('M-l', 'select-pane', '-t', '{bottom-right}');
  bind('M-]', 'run-shell', `${self} focus next`);
  bind('M-[', 'run-shell', `${self} focus prev`);
  for (let i = 1; i <= 9; i++) bind(`M-${i}`, 'run-shell', `${self} focus ${i}`);
  bind('M-n', 'select-pane', '-t', '{top-left}', ';', 'send-keys', '-t', '{top-left}', 'n');
  bind('M-m', 'select-pane', '-t', '{top-left}', ';', 'send-keys', '-t', '{top-left}', 'm');
  bind('M-d', 'detach-client');
}

/** Create the outer UI session (sidebar + nested attach). Idempotent. */
export function ensureUISession() {
  ensureAgentsSession();
  if (hasSession(UI)) return;
  tmux('new-session', '-d', '-s', UI, '-n', 'main', '-x', '200', '-y', '50', `${self} _attach-inner`);
  tmux('split-window', '-hb', '-l', '38', '-t', `${UI}:main`, `${self} ui`);
  tmux('set', '-t', UI, 'prefix', 'None');
  tmux('set', '-t', UI, 'prefix2', 'None');
  tmux('set', '-t', UI, 'status', 'on');
  tmux('set', '-t', UI, 'status-position', 'bottom');
  tmux('set', '-t', UI, 'status-style', 'bg=colour236,fg=colour250');
  tmux('set', '-t', UI, 'status-left-length', '250');
  tmux('set', '-t', UI, 'status-right', '');
  tmux('set', '-t', UI, 'window-status-format', '');
  tmux('set', '-t', UI, 'window-status-current-format', '');
  tmux(
    'set',
    '-t',
    UI,
    'status-left',
    ' #[bold,fg=colour214]agora#[default]  ' +
      '#[fg=colour245]M-n#[default] new  ' +
      '#[fg=colour245]M-m#[default] message  ' +
      '#[fg=colour245]M-←/→#[default] focus sidebar/agent  ' +
      '#[fg=colour245]M-[ M-]#[default] prev/next agent  ' +
      '#[fg=colour245]M-1..9#[default] jump  ' +
      '#[fg=colour245]M-d#[default] detach ',
  );
  tmux('set', '-t', `${UI}:main`, 'pane-border-style', 'fg=colour238');
  tmux('set', '-t', `${UI}:main`, 'pane-active-border-style', 'fg=colour214');
  tmux('select-pane', '-t', `${UI}:main.{bottom-right}`);
}

export function attach(session) {
  const env = { ...process.env };
  delete env.TMUX;
  const r = spawnSync('tmux', ['-L', SOCKET, 'attach-session', '-t', `=${session}`], { stdio: 'inherit', env });
  return r.status ?? 0;
}

export function listWindows(session = AGENTS) {
  const out = tmuxTry('list-windows', '-t', `=${session}`, '-F', '#{window_id}\t#{window_name}\t#{window_active}');
  if (!out) return [];
  return out
    .split('\n')
    .filter(Boolean)
    .map((l) => {
      const [id, name, active] = l.split('\t');
      return { id, name, active: active === '1' };
    });
}

export function activeWindowId(session = AGENTS) {
  return listWindows(session).find((w) => w.active)?.id ?? null;
}

export function capture(windowId, lines = 25) {
  return tmuxTry('capture-pane', '-p', '-t', windowId, '-S', `-${lines}`) ?? '';
}

export function selectWindow(windowId) {
  return tmuxTry('select-window', '-t', windowId) !== null;
}

export function killWindow(windowId) {
  return tmuxTry('kill-window', '-t', windowId) !== null;
}

export function windowExists(windowId) {
  return listWindows(AGENTS).some((w) => w.id === windowId);
}

/** Focus the agents pane (the right-hand pane) in the UI session, if the UI is running. */
export function focusAgentsPane() {
  if (!hasSession(UI)) return;
  tmuxTry('select-pane', '-t', `${UI}:main.{bottom-right}`);
}

export function focusSidebarPane() {
  if (!hasSession(UI)) return;
  tmuxTry('select-pane', '-t', `${UI}:main.{top-left}`);
}

/** Type `text` into the agent window as if pasted, then press Enter. */
export function typeInto(windowId, text) {
  tmux('send-keys', '-t', windowId, '-l', '--', text);
  // Small pause so the TUI has processed the paste before Enter arrives.
  spawnSync('sleep', ['0.35']);
  tmux('send-keys', '-t', windowId, 'Enter');
}

export function pressEnter(windowId) {
  tmuxTry('send-keys', '-t', windowId, 'Enter');
}

export function newWindow({ name, cwd, command }) {
  return tmux('new-window', '-d', '-t', `=${AGENTS}`, '-n', name, '-c', cwd, '-P', '-F', '#{window_id}', command);
}

export function detachAll(session = UI) {
  tmuxTry('detach-client', '-s', `=${session}`);
}

export function killServer() {
  tmuxTry('kill-server');
}
