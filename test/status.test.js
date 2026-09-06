import { test } from 'node:test';
import assert from 'node:assert/strict';
import { detectStatus, stripAnsi } from '../lib/status.js';

const cases = [
  ['claude trust dialog', ' Claude Code will be able to read, edit, and execute files here.\n ❯ No, exit\n   Yes, I trust this folder\n Enter to confirm · Esc to cancel', 'attention'],
  ['claude idle', '✻ Baked for 6s · done 2:35 PM\n─────── ada ─\n❯ \n──────\n  ⏵⏵ auto mode on (shift+tab to cycle) · ← for agents', 'idle'],
  ['claude working', '● Calling qoral 2 times · 33s…\n· Garnishing… (43s · ↓ 317 tokens)\n❯ \n  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt', 'working'],
  ['claude permission', '  Bash(rm -rf dist)\n  Do you want to proceed?\n ❯ 1. Yes\n   2. Yes, and don\'t ask again\n   3. No', 'attention'],
  ['codex login', '> 1. Sign in with ChatGPT\n  2. Sign in with Device Code\n  3. Provide your own API key\n  Press enter to continue', 'attention'],
  ['gemini mcp permission', '│ Allow execution of MCP tool "qoral_whoami" from server "qoral"?\n│ ● 1. Allow once\n│   2. Allow tool for this session', 'attention'],
  ['agy trust', 'Do you trust the contents of this project?\n> Yes, I trust this folder\n  No, exit\n  ↑/↓ Navigate · enter Confirm', 'attention'],
  ['agy idle', '────────\n>\n────────\n? for shortcuts                       Gemini 3.8 Flash · high', 'idle'],
  ['agy working', '⡿  Generating...\n────────\n>\n────────\nesc to cancel                         Gemini 3.8 Flash · high', 'working'],
  ['agy shell permission', 'Requesting permission for:\n   ls -la src/ test/\nDo you want to proceed?\n> 1. Yes\n  2. Yes, and always allow in this conversation\nesc to cancel', 'attention'],
  ['exited wrapper', '\n[qoral] agent "ada" exited (0). Press Enter to close this window.', 'exited'],
  ['documenting wrapper', '\n[qoral] documenting session… (writes .qoral/notes)', 'documenting'],
  ['empty screen', '\n\n', 'starting'],
  ['ansi is stripped', '\x1b[32m❯\x1b[0m \n  ? for shortcuts', 'idle'],
];

for (const [name, screen, expected] of cases) {
  test(`detectStatus: ${name}`, () => {
    assert.equal(detectStatus(screen), expected);
  });
}

test('stripAnsi removes CSI sequences', () => {
  assert.equal(stripAnsi('\x1b[1;32mhi\x1b[0m there\x1b[K'), 'hi there');
});
