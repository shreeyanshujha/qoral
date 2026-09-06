import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const BIN = fileURLToPath(new URL('../bin/agora.js', import.meta.url));
let tmp, env;

before(() => {
  tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'agora-mcp-'));
  env = { ...process.env, AGORA_HOME: path.join(tmp, 'home'), AGORA_SOCKET: `agoratest-mcp-${process.pid}` };
});
after(() => fs.rmSync(tmp, { recursive: true, force: true }));

/** Send JSON-RPC lines to `agora mcp` and collect the responses by id. */
function rpc(agent, messages) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [BIN, 'mcp', '--agent', agent], { env, cwd: tmp });
    let out = '';
    child.stdout.on('data', (d) => (out += d));
    child.on('error', reject);
    child.on('close', () => {
      const byId = {};
      for (const line of out.split('\n')) {
        if (!line.trim()) continue;
        const o = JSON.parse(line);
        byId[o.id] = o;
      }
      resolve(byId);
    });
    child.stdin.end(messages.map((m) => JSON.stringify(m)).join('\n') + '\n');
  });
}

const req = (id, method, params) => ({ jsonrpc: '2.0', id, method, params });
const call = (id, name, args = {}) => req(id, 'tools/call', { name, arguments: args });
const textOf = (r) => r.result.content[0].text;

test('initialize + tools/list', async () => {
  const r = await rpc('tester', [
    req(1, 'initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 't', version: '0' } }),
    { jsonrpc: '2.0', method: 'notifications/initialized' },
    req(2, 'tools/list'),
    req(3, 'ping'),
  ]);
  assert.equal(r[1].result.serverInfo.name, 'agora');
  const names = r[2].result.tools.map((t) => t.name);
  for (const n of ['agora_send', 'agora_inbox', 'agora_wait', 'agora_spawn', 'agora_list_agents', 'agora_log', 'agora_remember', 'agora_knowledge', 'agora_whoami']) {
    assert.ok(names.includes(n), `missing tool ${n}`);
  }
  assert.deepEqual(r[3].result, {});
});

test('send to human, log, unknown recipient error, unknown method', async () => {
  const r = await rpc('tester', [
    call(1, 'agora_send', { to: 'human', message: 'hello operator' }),
    call(2, 'agora_log', {}),
    call(3, 'agora_send', { to: 'nobody', message: 'x' }),
    req(4, 'no/such'),
    call(5, 'agora_whoami'),
  ]);
  assert.match(textOf(r[1]), /Queued for human/);
  assert.match(textOf(r[2]), /tester → human: hello operator/);
  assert.equal(r[3].result.isError, true);
  assert.match(textOf(r[3]), /no active agent named "nobody"/);
  assert.equal(r[4].error.code, -32601);
  assert.match(textOf(r[5]), /"tester"/);
});

test('agora_wait returns an already-queued message; inbox marks read', async () => {
  // "bob" is not a registered agent, so the CLI would refuse to address him; queue the row directly.
  process.env.AGORA_HOME = env.AGORA_HOME;
  const db = await import('../lib/db.js');
  db.addMessage('alice', 'bob', 'ping bob');
  const r = await rpc('bob', [call(1, 'agora_wait', { timeout_seconds: 5 }), call(2, 'agora_inbox')]);
  assert.match(textOf(r[1]), /alice → bob: ping bob/);
  assert.equal(textOf(r[2]), 'Inbox empty.');
});

test('agora_remember writes KNOWLEDGE.md in cwd and agora_knowledge reads it back', async () => {
  const r = await rpc('tester', [
    call(1, 'agora_remember', { text: 'CI runs on Node 26 only.' }),
    call(2, 'agora_knowledge'),
  ]);
  assert.match(textOf(r[1]), /Saved to .*KNOWLEDGE\.md/);
  assert.match(textOf(r[2]), /CI runs on Node 26 only/);
  assert.ok(fs.existsSync(path.join(tmp, '.agora', 'KNOWLEDGE.md')));
});
