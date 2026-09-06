// Theming. A theme maps semantic roles (accent, idle, working, …) and glyphs to colors/symbols.
// Colors accept: a base ANSI name ("red", "brightblue"), a 256-colour index (0-255 or "colour214"),
// or a hex string ("#ffaa00", truecolor). Each color yields both a terminal escape (sidebar) and a
// tmux color token (status bar). Resolution order:
//   QORAL_THEME env  >  config.json "theme"/"colors"/"glyphs"  >  built-in "default".
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { HOME } from './paths.js';

const E = '\x1b[';

// ---- built-in themes ----------------------------------------------------
// Each theme: { colors: {role: colorspec}, glyphs: {name: char} }. Roles left out fall back to default.

const BASE_GLYPHS = {
  idle: '●', working: ['◐', '◓', '◑', '◒'], attention: '!', documenting: '✎', moderating: '⚖',
  exited: '○', starting: '◌', cursor: '▸', unread: '↓', mail: '✉', bell: '•',
};

export const THEMES = {
  default: {
    colors: {
      accent: 214, // qoral orange
      idle: 'green', working: 'yellow', attention: 'red', documenting: 'cyan', moderating: 'cyan',
      exited: 'gray', starting: 'gray',
      text: 'white', dim: 'gray', border: 238, selection: 214, mail: 'magenta',
      claude: 214, codex: 'green', agy: 'magenta', gemini: 'blue', moderator: 'cyan',
      bar_fg: 250, bar_bg: 236, bar_key: 245,
    },
  },
  mono: {
    colors: {
      accent: 'white', idle: 'white', working: 245, attention: 'brightwhite', documenting: 245,
      moderating: 245, exited: 240, starting: 240, text: 'white', dim: 240, border: 238,
      selection: 'white', mail: 'white', claude: 'white', codex: 250, agy: 245, gemini: 250,
      moderator: 245, bar_fg: 250, bar_bg: 235, bar_key: 245,
    },
  },
  nord: {
    colors: {
      accent: '#88c0d0', idle: '#a3be8c', working: '#ebcb8b', attention: '#bf616a',
      documenting: '#b48ead', moderating: '#81a1c1', exited: '#4c566a', starting: '#4c566a',
      text: '#eceff4', dim: '#616e88', border: '#3b4252', selection: '#88c0d0', mail: '#b48ead',
      claude: '#d08770', codex: '#a3be8c', agy: '#b48ead', gemini: '#81a1c1', moderator: '#88c0d0',
      bar_fg: '#d8dee9', bar_bg: '#2e3440', bar_key: '#616e88',
    },
  },
  gruvbox: {
    colors: {
      accent: '#fabd2f', idle: '#b8bb26', working: '#fabd2f', attention: '#fb4934',
      documenting: '#d3869b', moderating: '#83a598', exited: '#665c54', starting: '#665c54',
      text: '#ebdbb2', dim: '#928374', border: '#3c3836', selection: '#fabd2f', mail: '#d3869b',
      claude: '#fe8019', codex: '#b8bb26', agy: '#d3869b', gemini: '#83a598', moderator: '#8ec07c',
      bar_fg: '#ebdbb2', bar_bg: '#282828', bar_key: '#928374',
    },
  },
  dracula: {
    colors: {
      accent: '#bd93f9', idle: '#50fa7b', working: '#f1fa8c', attention: '#ff5555',
      documenting: '#ff79c6', moderating: '#8be9fd', exited: '#6272a4', starting: '#6272a4',
      text: '#f8f8f2', dim: '#6272a4', border: '#44475a', selection: '#bd93f9', mail: '#ff79c6',
      claude: '#ffb86c', codex: '#50fa7b', agy: '#ff79c6', gemini: '#8be9fd', moderator: '#8be9fd',
      bar_fg: '#f8f8f2', bar_bg: '#282a36', bar_key: '#6272a4',
    },
  },
};

// ---- color parsing ------------------------------------------------------

const NAMED = {
  black: 0, red: 1, green: 2, yellow: 3, blue: 4, magenta: 5, cyan: 6, white: 7,
  gray: 8, grey: 8, brightblack: 8,
  brightred: 9, brightgreen: 10, brightyellow: 11, brightblue: 12, brightmagenta: 13, brightcyan: 14, brightwhite: 15,
};

function hexToRgb(hex) {
  const h = hex.replace('#', '');
  const s = h.length === 3 ? h.split('').map((c) => c + c).join('') : h;
  return [parseInt(s.slice(0, 2), 16), parseInt(s.slice(2, 4), 16), parseInt(s.slice(4, 6), 16)];
}

/** Parse a colorspec into { fg, bg, tmux } producers. Returns null on garbage. */
function parseColor(spec) {
  if (spec === null || spec === undefined) return null;
  if (typeof spec === 'number' && Number.isFinite(spec)) {
    const i = Math.max(0, Math.min(255, Math.round(spec)));
    return { fg: `${E}38;5;${i}m`, bg: `${E}48;5;${i}m`, tmux: `colour${i}` };
  }
  let s = String(spec).trim().toLowerCase();
  if (!s) return null;
  if (s.startsWith('#')) {
    const [r, g, b] = hexToRgb(s);
    if ([r, g, b].some(Number.isNaN)) return null;
    return { fg: `${E}38;2;${r};${g};${b}m`, bg: `${E}48;2;${r};${g};${b}m`, tmux: `#${s.replace('#', '').padStart(6, '0')}` };
  }
  if (s.startsWith('colour') || s.startsWith('color')) s = s.replace(/^colou?r/, '');
  if (/^\d+$/.test(s)) return parseColor(Number(s));
  if (s in NAMED) {
    const i = NAMED[s];
    return { fg: `${E}38;5;${i}m`, bg: `${E}48;5;${i}m`, tmux: `colour${i}` };
  }
  return null;
}

// ---- loading ------------------------------------------------------------

function loadUserThemeFiles() {
  // ~/.config/qoral/themes/<name>.json (and the classic ~/.config/qoral/theme.json = "custom")
  const dirs = [path.join(os.homedir(), '.config', 'qoral', 'themes'), path.join(HOME, 'themes')];
  const found = {};
  const single = path.join(os.homedir(), '.config', 'qoral', 'theme.json');
  try {
    if (fs.existsSync(single)) found.custom = JSON.parse(fs.readFileSync(single, 'utf8'));
  } catch {}
  for (const d of dirs) {
    let entries;
    try { entries = fs.readdirSync(d); } catch { continue; }
    for (const f of entries) {
      if (!f.endsWith('.json')) continue;
      try { found[f.replace(/\.json$/, '')] = JSON.parse(fs.readFileSync(path.join(d, f), 'utf8')); } catch {}
    }
  }
  return found;
}

function readConfig() {
  try {
    return JSON.parse(fs.readFileSync(path.join(HOME, 'config.json'), 'utf8'));
  } catch {
    return {};
  }
}

export function listThemes() {
  return [...new Set([...Object.keys(THEMES), ...Object.keys(loadUserThemeFiles())])].sort();
}

/**
 * Resolve the active theme into a usable object:
 *   S: sidebar style map (accent, idle, text, dim, … plus reset/bold/dim/inv/nobold)
 *   tmux: { fg, bg, key, accent, border } as tmux color tokens
 *   glyphs: symbol map (working is an array = spinner frames)
 *   name: resolved theme name
 */
export function loadTheme() {
  const cfg = readConfig();
  const userThemes = loadUserThemeFiles();
  const wanted = process.env.QORAL_THEME || cfg.theme || 'default';

  // Merge: default <- named builtin/user theme <- inline config.colors/glyphs
  const named = userThemes[wanted] || THEMES[wanted] || THEMES.default;
  const colors = { ...THEMES.default.colors, ...(named.colors || {}), ...(cfg.colors || {}) };
  const glyphs = { ...BASE_GLYPHS, ...(named.glyphs || {}), ...(cfg.glyphs || {}) };

  const col = (role) => parseColor(colors[role]) || parseColor(THEMES.default.colors[role]) || { fg: '', bg: '', tmux: 'default' };

  const S = {
    reset: `${E}0m`, bold: `${E}1m`, dim: `${E}2m`, inv: `${E}7m`, nobold: `${E}22m`,
    accent: col('accent').fg,
    text: col('text').fg,
    softdim: col('dim').fg,
    border: col('border').fg,
    selection: col('selection').fg,
    mail: col('mail').fg,
    idle: col('idle').fg,
    working: col('working').fg,
    attention: col('attention').fg,
    documenting: col('documenting').fg,
    moderating: col('moderating').fg,
    exited: col('exited').fg,
    starting: col('starting').fg,
    harness: {
      claude: col('claude').fg, codex: col('codex').fg, agy: col('agy').fg,
      gemini: col('gemini').fg, moderator: col('moderator').fg,
    },
  };

  const tmux = {
    fg: col('bar_fg').tmux, bg: col('bar_bg').tmux, key: col('bar_key').tmux,
    accent: col('accent').tmux, border: col('border').tmux,
  };

  return { name: userThemes[wanted] ? wanted : THEMES[wanted] ? wanted : 'default', S, tmux, glyphs };
}

/** Print the palette and glyphs for `agora doctor` / `agora theme`. */
export function describeThemes() {
  const active = loadTheme();
  const out = [`themes: ${listThemes().join(', ')}`, `active: ${active.name}`, ''];
  const swatch = (role) => {
    const c = active.S[role] || '';
    return `${c}██${active.S.reset} ${role}`;
  };
  out.push(['accent', 'idle', 'working', 'attention', 'documenting', 'exited'].map(swatch).join('   '));
  out.push(['claude', 'codex', 'agy', 'gemini', 'moderator'].map((h) => `${active.S.harness[h]}██${active.S.reset} ${h}`).join('   '));
  const g = active.glyphs;
  out.push(`glyphs: idle ${g.idle}  working ${Array.isArray(g.working) ? g.working.join('') : g.working}  attention ${g.attention}  documenting ${g.documenting}  exited ${g.exited}`);
  return out.join('\n');
}
