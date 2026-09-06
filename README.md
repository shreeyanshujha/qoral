# agora

Many coding agents, one terminal, and they can talk to each other.

agora is a Linux terminal workspace for running several AI coding agents side by side: Claude Code, OpenAI Codex, and Gemini CLI, in any mix, across any projects. A sidebar lists every agent with live status; the main pane shows whichever agent you select at full terminal fidelity. Every agent gets an `agora` MCP server so agents can message each other, broadcast, wait for replies, delegate by spawning sub-agents, and message you.

Zero dependencies beyond Node ≥ 22.13 and tmux ≥ 3.2, which are already on this machine.

```
┌─ agora ──────────────┬──────────────────────────────────────────────────┐
│ ▸1 ● ada     claude  │  ❯ [agora] message from grace: the API tests     │
│  2 ◐ grace   codex   │    fail on /users; can you check the model?      │
│  3 ! linus   gemini  │  ● Looking at models/user.js ...                 │
│                      │                                                  │
│ ~/Projects/api       │                                                  │
│ fix the failing…     │                                                  │
│ ── bus ───────────── │                                                  │
│ grace→ada the API…   │                                                  │
│ ada→human done, PR…  │                                                  │
│ n new m msg b bcast  │                                                  │
└──────────────────────┴──────────────────────────────────────────────────┘
```

## Install

```sh
ln -s ~/Projects/active/agora/bin/agora.js ~/.local/bin/agora
agora help
```

## Use

```sh
agora                              # open the workspace (creates it on first run)
agora spawn claude --dir ~/proj "write tests for lib/parse.js"
agora spawn codex  --dir ~/proj --name grace "review ada's tests when she pings you"
agora send ada "grace will review; ping her when done"
agora send all "wrap up in 10 minutes"
agora list                         # agents + status
agora log                          # bus traffic
agora kill grace
agora stop                         # tear everything down
```

### Sidebar keys

| key | action |
|---|---|
| `n` | new agent: harness, name, directory, task |
| `Enter` / `→` / `1-9` | show that agent in the main pane |
| `j` `k` `↑` `↓` | move selection |
| `m` | message the selected agent |
| `b` | broadcast to every agent |
| `x` | kill the selected agent (or remove an exited one) |
| `l` | full message log |
| `?` | help |
| `d` | detach; agents keep running, `agora` brings you back |
| `Q` | quit and kill every agent |

### From anywhere (also while typing into an agent)

| chord | action |
|---|---|
| `Alt+←` / `Alt+→` (or `Alt+h` / `Alt+l`) | focus sidebar / agent pane |
| `Alt+[` / `Alt+]` | previous / next agent |
| `Alt+1` … `Alt+9` | jump to agent N |
| `Alt+n` | new agent |
| `Alt+m` | message the selected agent |
| `Alt+d` | detach |

Status glyphs: `●` idle at prompt · `◐` working · `!` needs you (permission, trust or login dialog) · `○` exited.

## How agents talk

Each agent is launched with an `agora` MCP server (Claude via `--mcp-config`, Codex via `-c mcp_servers.agora...`, Gemini via a system-settings file). Tools:

| tool | purpose |
|---|---|
| `agora_list_agents` | who is running, their status and task |
| `agora_send(to, message)` | message an agent, `all`, or `human` |
| `agora_inbox()` | unread messages |
| `agora_wait(timeout_seconds, from?)` | block until a reply arrives |
| `agora_spawn(harness, name, cwd, prompt)` | delegate to a new agent |
| `agora_log(limit)` | recent bus traffic |

Messages live in SQLite at `~/.local/share/agora/agora.db`. The sidebar process runs the delivery loop: when a recipient is idle at its prompt, the pending message is typed into it as `[agora] message from X: …`, so the agent reacts without polling. Busy agents get it when they next go idle, or immediately if they call `agora_inbox` / `agora_wait`. Messages to `human` show up in the sidebar bus and in `agora log`.

Every agent's system prompt explains its name, who else is in the room, and the etiquette: be self-contained, coordinate before touching shared files, report back when delegated work is done.

## Notes and limits

- **First run per project**: Claude Code asks whether you trust the folder, Codex may ask you to sign in, Gemini may ask about tool permissions. agora flags these as `!` so you can answer them.
- **Status is heuristic**: it reads the last screenful of each pane. Unusual prompts may briefly show as "working".
- **Alt chords** are bound in tmux's root key table and take precedence over the same chords inside agents.
- Everything runs on a private tmux socket (`-L agora`), so your own tmux sessions and config are untouched.
- `AGORA_HOME` and `AGORA_SOCKET` env vars relocate the data directory and socket, useful for a second isolated workspace.

## Layout

```
bin/agora.js     entry point
lib/cli.js       commands
lib/tmux.js      private tmux server, sessions, key bindings
lib/agents.js    spawn / focus / kill / send
lib/harness.js   per-CLI launch commands and MCP wiring
lib/mcp.js       the stdio MCP server agents talk through
lib/bus.js       status detection + message delivery loop
lib/status.js    pane-text heuristics
lib/ui.js        sidebar TUI
lib/db.js        SQLite (node:sqlite)
```
