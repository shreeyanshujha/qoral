#!/usr/bin/env node
// Silence node:sqlite / experimental warnings so they never garble the TUI or MCP stream.
process.removeAllListeners('warning');
process.on('warning', () => {});

const { main } = await import('../lib/cli.js');
main(process.argv.slice(2)).catch((err) => {
  process.stderr.write(`agora: ${err.message}\n`);
  process.exit(1);
});
