import { test } from 'node:test';
import assert from 'node:assert/strict';
import { THEMES, listThemes, loadTheme } from '../lib/theme.js';

test('every built-in theme resolves to complete styles', () => {
  for (const name of Object.keys(THEMES)) {
    process.env.QORAL_THEME = name;
    const t = loadTheme();
    assert.equal(t.name, name);
    for (const role of ['accent', 'idle', 'working', 'attention', 'text', 'border']) {
      assert.equal(typeof t.S[role], 'string', `${name}.${role} missing`);
    }
    for (const h of ['claude', 'codex', 'agy', 'gemini', 'moderator']) {
      assert.equal(typeof t.S.harness[h], 'string', `${name} harness ${h} missing`);
    }
    for (const key of ['fg', 'bg', 'key', 'accent', 'border']) {
      assert.match(t.tmux[key], /^(colour\d+|#[0-9a-f]{6}|default)$/, `${name}.tmux.${key} = ${t.tmux[key]}`);
    }
  }
  delete process.env.QORAL_THEME;
});

test('unknown theme falls back to default', () => {
  process.env.QORAL_THEME = 'does-not-exist';
  assert.equal(loadTheme().name, 'default');
  delete process.env.QORAL_THEME;
});

test('hex colors produce truecolor escapes, indices produce 256-color', () => {
  process.env.QORAL_THEME = 'nord'; // hex-based
  assert.match(loadTheme().S.accent, /\x1b\[38;2;\d+;\d+;\d+m/);
  process.env.QORAL_THEME = 'default'; // index/name based
  assert.match(loadTheme().S.accent, /\x1b\[38;5;\d+m/);
  delete process.env.QORAL_THEME;
});

test('listThemes includes the built-ins', () => {
  const names = listThemes();
  for (const n of ['default', 'mono', 'nord', 'gruvbox', 'dracula']) assert.ok(names.includes(n));
});
