# Contributing

qoral is a single Rust binary: a daemon that owns the agents' pseudo-terminals and a TUI client that attaches to it. No runtime dependencies beyond the agent CLIs you already use.

## Setup

```sh
git clone https://github.com/shreeyanshujha/qoral.git
cd qoral
cargo build
cargo test                      # unit tests + integration tests (tests/e2e.rs)
./target/debug/qoral doctor
```

Rust 1.85 or newer. If you use `mise`: `mise use rust@stable`.

## Tests

`cargo test` runs the status heuristics and two integration tests that start a real daemon against a throwaway `QORAL_HOME`, with a fake `claude` from `test/fixtures/` on `PATH`. The fake behaves like a prompt, logs whatever is typed into it, and exits on `/exit`, so the tests exercise PTY spawning, terminal emulation, status detection, message nudging, the MCP server, `type`, `kill`, and `stop` without any real agent. Real-agent behaviour changes (a new prompt glyph, a new permission dialog) go into `src/status.rs` with a case in its test.

To try changes against real agents without touching your daily workspace:

```sh
QORAL_HOME=/tmp/qoral-dev ./target/debug/qoral
```

Each `QORAL_HOME` gets its own daemon and socket. After rebuilding, the next client notices the daemon is an older build and restarts it if no agents are running; with agents running, `qoral stop` it yourself.

## Layout

```
src/main.rs          CLI (clap) and command dispatch
src/daemon/mod.rs    daemon: clients, housekeeping tick, nudges, documentation triggers
src/daemon/agent.rs  one agent: PTY (portable-pty) + emulator (alacritty_terminal) + frames
src/client/mod.rs    TUI event loop, modes, wizards
src/client/ui.rs     drawing (ratatui)
src/client/keys.rs   key / mouse / paste → terminal bytes
src/proto.rs         client↔daemon messages (MessagePack, length-prefixed)
src/ctl.rs           connect / auto-start / one-shot requests
src/db.rs            SQLite bus and agent registry (rusqlite)
src/mcp.rs           the MCP server agents talk through (stdio JSON-RPC)
src/harness.rs       how each CLI is launched and wired to MCP
src/status.rs        screen-text heuristics
src/knowledge.rs     transcripts, summarizer, session notes, KNOWLEDGE.md
src/debate.rs        moderated debates → .qoral/decisions/
src/theme.rs         themes
src/doctor.rs        `qoral doctor`
tests/e2e.rs         integration tests
test/fixtures/claude fake harness used by tests
```

Rough rules:

- `daemon/agent.rs` is the only file that knows about PTYs and the emulator.
- `harness.rs` is the only file that knows how each CLI is launched and how its MCP server is wired.
- Anything that edits a user's config files must be idempotent and must return notices describing what it changed.
- No `.await` while holding the daemon's `State` mutex.

## Adding a harness

1. Add its name to `HARNESSES` in `src/paths.rs` and the doctor's label list.
2. Add a `match` arm to `build_launch` in `src/harness.rs`: MCP wiring, how the briefing is passed (system-prompt flag or initial prompt), how identity reaches the MCP server (`--agent` arg or the `QORAL_AGENT` env var).
3. Extend `src/status.rs` if its prompt / spinner / dialogs aren't recognised, with tests.
4. Add a transcript source in `src/knowledge.rs` if the CLI keeps session files.
5. Add a colour role in `src/theme.rs` and a row to the harness table in the README.

## Pull requests

- `cargo test` and `cargo clippy -- -D warnings` clean.
- Describe what you tested against real agents, since CI only runs the fake one.
- Update `CHANGELOG.md` under *Unreleased*.
