# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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
