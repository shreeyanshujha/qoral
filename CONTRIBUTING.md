# Contributing

Thanks for helping. qoral is small on purpose: plain Node (≥ 22.13), no dependencies, no build step, tmux as the multiplexer.

## Setup

```sh
git clone https://github.com/shreeyanshujha/qoral.git
cd qoral
./install.sh          # links bin/qoral.js onto your PATH
qoral doctor
npm test
```

## Tests

`npm test` runs `node --test`. The suite does not need any real agent CLI: `test/fixtures/` contains a fake `claude` that behaves like a prompt, and the tests run a throwaway workspace on a private tmux socket with `QORAL_HOME` pointed at a temp directory. Tests that need tmux skip themselves when it is missing.

To try changes against real agents without touching your daily workspace:

```sh
QORAL_HOME=/tmp/qoral-dev QORAL_SOCKET=qoraldev qoral
```

## Layout

See the "Layout" section of the README. Rough rules:

- `lib/tmux.js` is the only file that talks to tmux.
- `lib/harness.js` is the only file that knows how each CLI is launched and how its MCP server is wired.
- `lib/status.js` holds the screen-text heuristics. When a CLI changes its prompt or spinner wording, fix it there and add a case to `test/status.test.js`.
- Anything that edits a user's config files must be idempotent and must report what it changed (see `ensureAgyIntegration`).

## Adding a harness

1. Add its name to `HARNESSES` and `HARNESS_LABELS` in `lib/paths.js`.
2. Add a `case` to `buildLaunch` in `lib/harness.js`: how to pass the MCP server, how to pass the briefing (system prompt flag or initial prompt), how identity reaches the MCP server (`--agent` arg or `QORAL_AGENT` env).
3. Add idle / working / attention patterns to `lib/status.js` if the defaults miss them, with tests.
4. Add a transcript source to `lib/transcript.js` if the CLI keeps session files.
5. Add a colour tag in `lib/ui.js` and a row to the harness table in the README.

## Pull requests

- Keep zero dependencies.
- Run `npm run check && npm test` before pushing.
- Describe what you tested against real agents, since CI only runs the fake one.
- Update `CHANGELOG.md` under *Unreleased*.
