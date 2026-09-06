// End-to-end test of the tmux workspace with a fake `claude` on PATH:
// spawn → status detection → message nudge typed into the prompt → MCP send → kill → stop.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const BIN = fileURLToPath(new URL('../bin/agora.js', import.meta.url));
const FIXTURES = fileURLToPath(new URL('./fixtures/', import.meta.url));
const hasTmux = spawnSync('tmux', ['-V']).status === 0;

let tmp, env, proj, fakeLog;
const agora = (...args) => execFileSync(process.execPath, [BIN, ...args], { env, encoding: 'utf8', cwd: proj });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function waitFor(pred, ms = 20000, step = 400) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (pred()) return true;
    await sleep(step);
  }
  return pred();
}
const logText = () => (fs.existsSync(fakeLog) ? fs.readFileSync(fakeLog, 'utf8') : '');

before(() => {
  tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'agora-ws-'));
  proj = path.join(tmp, 'proj');
  fs.mkdirSync(proj);
  fakeLog = path.join(tmp, 'fake.log');
  fs.chmodSync(path.join(FIXTURES, 'claude'), 0o755);
  env = {
    ...process.env,
    AGORA_HOME: path.join(tmp, 'home'),
    AGORA_SOCKET: `agoratest-ws-${process.pid}`,
    AGORA_SUMMARIZER: 'none',
    FAKE_LOG: fakeLog,
    PATH: `${FIXTURES}:${process.env.PATH}`,
  };
});

after(() => {
  if (hasTmux) spawnSync('tmux', ['-L', env.AGORA_SOCKET, 'kill-server']);
  fs.rmSync(tmp, { recursive: true, force: true });
});

test('workspace: spawn, idle detection, nudge delivery, MCP roundtrip, kill, stop', { skip: !hasTmux && 'tmux not installed' }, async () => {
  agora('up'); // sidebar + bus running detached
  const sessions = spawnSync('tmux', ['-L', env.AGORA_SOCKET, 'list-sessions', '-F', '#{session_name}'], { encoding: 'utf8' }).stdout;
  assert.match(sessions, /agents/);
  assert.match(sessions, /agora/);

  const out = agora('spawn', 'claude', '--dir', proj, '--name', 'fake', '--no-focus', 'do the thing');
  assert.match(out, /started fake \(claude\)/);

  // the fake harness received the MCP wiring and the task
  assert.ok(await waitFor(() => /ARGS: [\s\S]*--mcp-config [\s\S]*do the thing/.test(logText())), `fake never started: ${logText().slice(0, 300)}`);

  // status detection sees the prompt
  assert.ok(await waitFor(() => /fake\s+claude\s+idle/.test(agora('list'))), `not idle: ${agora('list')}`);

  // a queued message is typed into the idle prompt by the bus
  agora('send', 'fake', 'hello from the test');
  assert.ok(await waitFor(() => /INPUT: \[agora\] message from human: hello from the test/.test(logText())), `nudge not delivered: ${logText()}`);

  // an agent can talk back through the MCP server
  const mcp = spawnSync(process.execPath, [BIN, 'mcp', '--agent', 'fake'], {
    env,
    encoding: 'utf8',
    input: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name: 'agora_send', arguments: { to: 'human', message: 'done' } } }) + '\n',
  });
  assert.match(mcp.stdout, /Queued for human/);
  assert.match(agora('log', '-n', '5'), /fake → human: done/);

  // kill removes the agent and the window
  agora('kill', 'fake', '--no-docs');
  assert.doesNotMatch(agora('list'), /^fake\s/m);

  agora('stop');
  assert.equal(spawnSync('tmux', ['-L', env.AGORA_SOCKET, 'has-session', '-t', 'agents']).status !== 0, true);
});
