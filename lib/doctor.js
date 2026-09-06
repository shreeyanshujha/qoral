// `qoral doctor`: check the machine for everything qoral needs and print platform-specific advice.
import fs from 'node:fs';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import { HARNESSES, HARNESS_LABELS, BIN, HOME, VERSION } from './paths.js';

const G = '\x1b[32m', Y = '\x1b[33m', R = '\x1b[31m', D = '\x1b[2m', B = '\x1b[1m', X = '\x1b[0m';
const ok = (s) => `${G}✔${X} ${s}`;
const warn = (s) => `${Y}!${X} ${s}`;
const bad = (s) => `${R}✘${X} ${s}`;

export const MIN_NODE = [22, 13, 0];
export const MIN_TMUX = [3, 2];

function run(cmd, args = []) {
  const r = spawnSync(cmd, args, { encoding: 'utf8' });
  return r.status === 0 ? (r.stdout || r.stderr || '').trim() : null;
}
export const which = (cmd) => run('sh', ['-c', `command -v ${cmd}`]);

export function versionAtLeast(actual, min) {
  const a = String(actual).match(/\d+/g)?.map(Number) ?? [];
  for (let i = 0; i < min.length; i++) {
    const x = a[i] ?? 0;
    if (x > min[i]) return true;
    if (x < min[i]) return false;
  }
  return true;
}

export function isWSL() {
  if (process.platform !== 'linux') return false;
  try {
    return /microsoft/i.test(fs.readFileSync('/proc/version', 'utf8'));
  } catch {
    return false;
  }
}

export function platformName() {
  if (process.platform === 'darwin') return 'macOS';
  if (process.platform === 'win32') return 'Windows (native)';
  if (isWSL()) return 'Windows (WSL2)';
  return 'Linux';
}

/** Does the terminfo database know this terminal name? */
export function hasTerminfo(name) {
  return spawnSync('infocmp', [name], { stdio: 'ignore' }).status === 0;
}

export function runDoctor() {
  const lines = [];
  let problems = 0;
  const p = platformName();
  lines.push(`${B}qoral ${VERSION}${X} · ${p} · ${os.release()} · ${BIN}`);
  lines.push('');

  // node
  const nodeOk = versionAtLeast(process.versions.node, MIN_NODE);
  lines.push(nodeOk ? ok(`node ${process.versions.node}`) : bad(`node ${process.versions.node}; need ≥ ${MIN_NODE.join('.')} for node:sqlite`));
  if (!nodeOk) problems++;
  if (process.getBuiltinModule?.('node:sqlite')) lines.push(ok('node:sqlite available'));
  else {
    lines.push(bad('node:sqlite not available in this Node build'));
    problems++;
  }

  // tmux
  if (process.platform === 'win32') {
    lines.push(bad('tmux: not available on native Windows. Run qoral inside WSL2 (see below).'));
    problems++;
  } else {
    const tv = run('tmux', ['-V']);
    if (!tv) {
      lines.push(bad(`tmux not found. Install: ${process.platform === 'darwin' ? 'brew install tmux' : 'sudo apt install tmux / sudo pacman -S tmux / sudo dnf install tmux'}`));
      problems++;
    } else if (!versionAtLeast(tv, MIN_TMUX)) {
      lines.push(bad(`${tv}; need ≥ ${MIN_TMUX.join('.')}`));
      problems++;
    } else lines.push(ok(tv));
    const ti = hasTerminfo('tmux-256color');
    lines.push(ti ? ok('terminfo: tmux-256color') : warn(`terminfo lacks tmux-256color; qoral will use screen-256color inside tmux${process.platform === 'darwin' ? ' (fix: brew install ncurses, or add the entry with tic)' : ''}`));
  }

  // terminal
  const term = process.env.TERM || '(unset)';
  const colorterm = process.env.COLORTERM ? ` COLORTERM=${process.env.COLORTERM}` : '';
  const termProgram = process.env.TERM_PROGRAM ? ` (${process.env.TERM_PROGRAM})` : process.env.WT_SESSION ? ' (Windows Terminal)' : '';
  lines.push(ok(`terminal: TERM=${term}${colorterm}${termProgram}`));

  // harnesses
  lines.push('');
  lines.push(`${B}agent CLIs${X}`);
  let found = 0;
  for (const h of HARNESSES) {
    const w = which(h);
    if (w) {
      found++;
      const v = run(h, ['--version'])?.split('\n')[0] ?? '';
      lines.push(ok(`${h.padEnd(7)} ${HARNESS_LABELS[h]}  ${D}${v}${X}`));
    } else lines.push(warn(`${h.padEnd(7)} ${HARNESS_LABELS[h]}  ${D}not on PATH${X}`));
  }
  if (!found) {
    lines.push(bad('no agent CLIs found; install at least one (claude, codex, agy, gemini)'));
    problems++;
  }

  // data dir
  lines.push('');
  lines.push(`${B}data${X}`);
  lines.push(ok(`workspace data: ${HOME}`));
  const cfg = `${HOME}/config.json`;
  lines.push(fs.existsSync(cfg) ? ok(`config: ${cfg}`) : warn(`config: ${cfg} ${D}(not created; defaults in use)${X}`));

  // platform notes
  lines.push('');
  lines.push(`${B}notes for ${p}${X}`);
  if (process.platform === 'darwin') {
    lines.push(`${D}- Alt chords need Option to send Meta: Terminal.app → Settings → Profiles → Keyboard → "Use Option as Meta key";${X}`);
    lines.push(`${D}  iTerm2 → Profiles → Keys → Left Option key: Esc+. Or use the sidebar keys (n, m, D, l …) instead.${X}`);
    lines.push(`${D}- Install deps: brew install node tmux${X}`);
  } else if (process.platform === 'win32') {
    lines.push(`${D}- qoral's multiplexer is tmux, which has no native Windows build. Use WSL2:${X}`);
    lines.push(`${D}    wsl --install            (once, then reboot)${X}`);
    lines.push(`${D}    then inside WSL: install node ≥ 22.13 and tmux, clone qoral, run ./install.sh${X}`);
    lines.push(`${D}- windows\\install.ps1 puts an "qoral" command on your PowerShell PATH that runs it inside WSL.${X}`);
  } else if (isWSL()) {
    lines.push(`${D}- Windows Terminal binds Alt+arrows to pane focus; use Alt+h / Alt+l in qoral, or unbind them in Windows Terminal settings.${X}`);
    lines.push(`${D}- Keep your projects on the Linux filesystem (~/…), not /mnt/c, for speed.${X}`);
  } else {
    lines.push(`${D}- Any terminal works (Foot, Alacritty, Kitty, GNOME Terminal, Konsole …); qoral runs inside tmux.${X}`);
    lines.push(`${D}- If Alt chords are eaten by your window manager, use the sidebar keys or rebind in lib/tmux.js.${X}`);
  }

  lines.push('');
  lines.push(problems ? bad(`${problems} problem(s) to fix before qoral will run`) : ok('ready: run `qoral`'));
  console.log(lines.join('\n'));
  return problems;
}
