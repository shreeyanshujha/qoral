# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.2] - 2026-09-07

### Added
- OpenCode harness (`opencode`): any provider OpenCode supports (DeepSeek, OpenRouter, local models …). Per-agent config via `OPENCODE_CONFIG` with the qoral MCP server merged into a copy of your `opencode.json`; `"opencode_model"` in qoral's config picks the model; transcripts via `opencode export`. Not seated in debates automatically; use `--agents` or `debate_harnesses`.
- After an upgrade or rebuild, clients detect that the running daemon is a different build and restart it when no agents are running, instead of failing with protocol errors.
- `qoral debate --count N` (`-n`) chooses how many participants (2–4) without naming harnesses; the `D` wizard asks for participants (a number or a harness list).
- Config `debate_harnesses` (harness pool) and `debate_count` (default size).

### Changed
- Default debate participants are drawn from *usable* harnesses: Codex is skipped when `~/.codex/auth.json` is missing, so an unsigned Codex no longer eats a seat and stalls a round.

## [0.3.1] - 2026-09-07

### Added
- `qoral debate --options`: participants develop distinct candidates instead of converging; the write-up is a per-option comparison with an optional recommendation, linked from KNOWLEDGE.md as undecided and announced on the bus as `OPTIONS:`.
- `qoral build <decisions-file> [--option N]`: spawn a builder for a decision document or for one option of an options document.
- The sidebar's `D` wizard asks for "decide" or "options".

## [0.3.0] - 2026-09-06

Rewritten in Rust. qoral is now a single binary that is its own multiplexer; tmux and Node are no longer needed.

### Changed
- **Architecture.** A daemon owns every agent in a pseudo-terminal (portable-pty) with a full terminal emulator per agent (alacritty_terminal) and 20k lines of scrollback. Clients attach over a Unix socket and receive screen frames at up to 60 fps. Sessions survive closing the terminal; several clients can attach.
- **Status detection** reads the emulator's screen grid directly instead of scraping tmux.
- **Documentation transcripts** add the emulator's own scrollback and a raw PTY log as fallbacks.
- `qoral kill` / `x` document the session in the background; `qoral document <name>` snapshots a running agent.
- The socket lives in `$XDG_RUNTIME_DIR/qoral/` (keyed by `QORAL_HOME`), so deep data paths no longer break the Unix socket length limit.

### Added
- Theming: built-in themes (`default`, `mono`, `nord`, `gruvbox`, `dracula`), user theme files under `~/.local/share/qoral/themes/` or `~/.config/qoral/themes/`, inline `colors`/`glyphs` overrides in `config.json`. `qoral theme` previews; `qoral theme init` scaffolds.
- `qoral up`, `qoral type <agent> <text>`, `qoral screen <agent>` for scripting and tests.
- Editor and debate windows inside the workspace (`o`, `D`); they close themselves when done.
- Integration tests (`cargo test`) that drive a real daemon with a fake harness.
- Prebuilt binaries for Linux (x86_64, aarch64) and macOS (x86_64, aarch64) on each release; `install.sh` prefers them and falls back to a cargo build.

### Removed
- The Node.js implementation and its tmux dependency.

## [0.2.0] - 2026-09-06

First public beta. Linux and macOS.

### Added
- Sidebar TUI plus a nested tmux session: one window per agent, full terminal fidelity, Alt chords for switching.
- Four harnesses: Claude Code, OpenAI Codex, Google Antigravity CLI (`agy`), Gemini CLI.
- `qoral` MCP server given to every agent: `qoral_send`, `qoral_inbox`, `qoral_wait`, `qoral_spawn`, `qoral_list_agents`, `qoral_log`, `qoral_remember`, `qoral_knowledge`.
- Message bus in SQLite; pending messages are typed into idle agents' prompts, so conversations flow without polling.
- Living documentation: on exit or kill, each session is summarized into `<project>/.qoral/notes/` and merged into `<project>/.qoral/KNOWLEDGE.md`, which every future agent in that project receives.
- `qoral debate "<question>"`: moderated multi-agent debates that end in a decision document under `.qoral/decisions/`, with optional `--build`.
- `qoral doctor`, `install.sh`, AUR and Homebrew packaging templates.
- Windows via WSL2 launchers (`windows/`), unofficial for now.

### Security
- Agent-initiated spawning (`qoral_spawn`) is capped by `max_agents` (default 8) in `~/.local/share/qoral/config.json`.
- qoral prints a notice the first time it edits a harness's config files (Antigravity MCP registration and permission rules).
