#!/usr/bin/env node
// Silence node:sqlite / experimental warnings so they never garble the TUI or MCP stream.
process.removeAllListeners('warning');
process.on('warning', () => {});

// ---- platform gate ----------------------------------------------------------
const [maj, min] = process.versions.node.split('.').map(Number);
if (maj < 22 || (maj === 22 && min < 13)) {
  process.stderr.write(`qoral: Node ${process.versions.node} is too old; need ≥ 22.13 (for the built-in node:sqlite).\n`);
  process.exit(1);
}

if (process.platform === 'win32' && !process.argv.slice(2).some((a) => ['doctor', 'help', '--help', '-h', 'version', '--version', '-v'].includes(a))) {
  process.stderr.write(
    [
      'qoral: native Windows is not supported, because qoral multiplexes agents with tmux and tmux has no Windows build.',
      '',
      'Run it inside WSL2 instead (Windows Terminal and PowerShell both work with it):',
      '  1. wsl --install                       # once; then reboot',
      '  2. inside WSL: install node >= 22.13 and tmux, clone qoral, run ./install.sh',
      '  3. from PowerShell: powershell -ExecutionPolicy Bypass -File windows\\install.ps1',
      '     -> gives you an `qoral` command in PowerShell that runs inside WSL',
      '',
      'Run `qoral doctor` for details.',
      '',
    ].join('\n'),
  );
  process.exit(1);
}

const { main } = await import('../lib/cli.js');
main(process.argv.slice(2)).catch((err) => {
  process.stderr.write(`qoral: ${err.message}\n`);
  process.exit(1);
});
