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
- `agora` MCP server given to every agent: `agora_send`, `agora_inbox`, `agora_wait`, `agora_spawn`, `agora_list_agents`, `agora_log`, `agora_remember`, `agora_knowledge`.
- Message bus in SQLite; pending messages are typed into idle agents' prompts, so conversations flow without polling.
- Living documentation: on exit or kill, each session is summarized into `<project>/.agora/notes/` and merged into `<project>/.agora/KNOWLEDGE.md`, which every future agent in that project receives.
- `agora debate "<question>"`: moderated multi-agent debates that end in a decision document under `.agora/decisions/`, with optional `--build`.
- `agora doctor`, `install.sh`, AUR and Homebrew packaging templates.
- Windows via WSL2 launchers (`windows/`), unofficial for now.

### Security
- Agent-initiated spawning (`agora_spawn`) is capped by `max_agents` (default 8) in `~/.local/share/agora/config.json`.
- agora prints a notice the first time it edits a harness's config files (Antigravity MCP registration and permission rules).
