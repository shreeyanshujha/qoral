# agora

Many coding agents, one terminal, they talk to each other, and every session leaves documentation behind.

agora is a Linux terminal workspace for running several AI coding agents side by side: Claude Code, OpenAI Codex, Google Antigravity CLI (`agy`), and Gemini CLI, in any mix, across any projects. A sidebar lists every agent with live status; the main pane shows whichever agent you select at full terminal fidelity. Every agent gets an `agora` MCP server so agents can message each other, broadcast, wait for replies, delegate by spawning sub-agents, and message you. When a session ends it is summarized into a per-project knowledge base that every future agent in that project receives.

Zero dependencies beyond Node ≥ 22.13 and tmux ≥ 3.2. Runs on Linux, macOS, and Windows via WSL2, in any terminal.

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
git clone https://github.com/shreeyanshujha/agora.git
cd agora && ./install.sh     # checks node/tmux, links `agora` onto your PATH
agora doctor                 # verifies everything, prints platform tips
```

`npm install -g .` in the clone works too. There is nothing to build and no `node_modules`. Packaging templates for the AUR and Homebrew live in `packaging/`.

## Platforms

| platform | how | notes |
|---|---|---|
| **Linux** | `./install.sh` | Any terminal: Foot, Alacritty, Kitty, GNOME Terminal, Konsole, xterm. agora draws inside tmux, so the terminal only needs UTF-8 and 256 colors. |
| **macOS** | `brew install node tmux`, then `./install.sh` | Alt chords need Option to send Meta: Terminal.app → Settings → Profiles → Keyboard → *Use Option as Meta key*; iTerm2 → Profiles → Keys → *Left Option key: Esc+*. Without it, use the sidebar keys. macOS's terminfo lacks `tmux-256color`; agora detects that and uses `screen-256color`. |
| **Windows** | WSL2 + `windows\install.ps1` | tmux has no native Windows build, so agora runs inside WSL2. The PowerShell installer runs `install.sh` inside your distro and puts an `agora` command on your Windows PATH that forwards to WSL, so `agora`, `agora spawn …`, `agora debate …` work from PowerShell, cmd, and Windows Terminal. Windows Terminal binds Alt+arrows to its own panes; use `Alt+h` / `Alt+l` in agora or unbind them. Keep projects on the Linux filesystem, not `/mnt/c`, for speed. |

Windows, step by step:

```powershell
wsl --install                       # once; reboot
# inside WSL: install node >= 22.13 and tmux, e.g. on Ubuntu:
#   sudo apt install -y tmux && curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash - && sudo apt install -y nodejs
#   git clone https://github.com/shreeyanshujha/agora.git ~/agora
# back in PowerShell:
powershell -ExecutionPolicy Bypass -File \\wsl$\Ubuntu\home\<you>\agora\windows\install.ps1
agora doctor
```

The agent CLIs (Claude Code, Codex, agy, Gemini) also have to be installed inside WSL, since that's where agora launches them.

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
agora debate "how should divide() handle division by zero?" --dir ~/proj --build
agora stop                         # tear everything down
```

### Sidebar keys

| key | action |
|---|---|
| `n` | new agent: harness, name, directory, task |
| `D` | debate a question with three agents, optionally build the result |
| `Enter` / `→` / `1-9` | show that agent in the main pane |
| `j` `k` `↑` `↓` | move selection |
| `m` | message the selected agent |
| `b` | broadcast to every agent |
| `x` | kill the selected agent (or remove an exited one) |
| `l` | full message log |
| `o` | open the project's `.agora/KNOWLEDGE.md` |
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

Status glyphs: `●` idle at prompt · `◐` working · `!` needs you (permission, trust or login dialog) · `✎` documenting · `○` exited.

## Supported harnesses

| harness | command | MCP wiring | identity | permission pre-approval |
|---|---|---|---|---|
| `claude` | Claude Code | `--mcp-config` per agent | `--agent` arg | `--allowedTools mcp__agora` |
| `codex` | OpenAI Codex | `-c mcp_servers.agora...` per agent | `--agent` arg | follows your Codex approval policy |
| `agy` | Google Antigravity CLI | global `~/.gemini/config/mcp_config.json`, merged idempotently | `AGORA_AGENT` env (agy passes env through) | `mcp(agora/<tool>)` rules merged into `~/.gemini/antigravity-cli/settings.json` |
| `gemini` | Gemini CLI | per-agent system-settings file via `GEMINI_CLI_SYSTEM_SETTINGS_PATH` | `--agent` arg | `trust: true` on the server |

agy has no per-session MCP flag and no system-prompt flag, so agora registers the server once in your agy config (and pre-approves its tools) and sends the agent's briefing as the initial prompt. Both edits are additive and idempotent; `agy mcp list` shows the entry.

## How agents talk

Each agent is launched with an `agora` MCP server. Tools:

| tool | purpose |
|---|---|
| `agora_list_agents` | who is running, their status and task |
| `agora_send(to, message)` | message an agent, `all`, or `human` |
| `agora_inbox()` | unread messages |
| `agora_wait(timeout_seconds, from?)` | block until a reply arrives |
| `agora_spawn(harness, name, cwd, prompt)` | delegate to a new agent |
| `agora_log(limit)` | recent bus traffic |
| `agora_remember(text)` | save a durable project fact for future agents |
| `agora_knowledge()` | read the project digest and note list |

Messages live in SQLite at `~/.local/share/agora/agora.db`. The sidebar process runs the delivery loop: when a recipient is idle at its prompt, the pending message is typed into it as `[agora] message from X: …`, so the agent reacts without polling. Busy agents get it when they next go idle, or immediately if they call `agora_inbox` / `agora_wait`. Messages to `human` show up in the sidebar bus and in `agora log`.

Every agent's system prompt explains its name, who else is in the room, and the etiquette: be self-contained, coordinate before touching shared files, report back when delegated work is done.

## Debates: let agents argue it out

```sh
agora debate "Should we move the tests to node:test, and how?" --dir ~/proj --build
```

agora spawns three participants with different perspectives (a pragmatist, a skeptic, a minimalist; a fourth, an architect, if you ask for four) on different harnesses, and moderates them itself over the bus. The moderator is a deterministic process, not another model, so the protocol is enforced:

1. Each participant reads the code, then sends an opening position to `moderator` ending in `STANCE:` and `CONSENSUS:` lines.
2. Each round the moderator relays everyone's positions to everyone else. Participants critique, concede or hold, and reply.
3. It stops when every participant writes `CONSENSUS: yes`, or after the round limit.
4. The summarizer writes a decision document to `.agora/decisions/<date>-<slug>.md`: decision or recommendation, rationale, alternatives considered, dissent and risks, a numbered implementation plan, and the full transcript. A one-line link is added under `## Decisions` in `KNOWLEDGE.md`, so every future agent inherits the outcome.
5. With `--build`, a builder agent implements the plan, runs the tests, and reports to you.

Flags: `--agents claude,agy,codex` (2 to 4, repeats allowed), `--rounds 3`, `--timeout 300` seconds per round, `--keep` to leave participants running for follow-up questions. From the sidebar, `D` starts a debate in a new window so you can watch the moderator's log.

In a live run, two Claude participants and one agy participant argued a testing-structure question for two rounds, the minimalist and pragmatist converged with explicit concessions, the decision document cited the project's zero-dependency rule and Node version from the knowledge digest, and the builder implemented it with passing tests. Participants stuck on a permission prompt are flagged in the moderator log and on the bus, since a silent participant is the main way a round runs to its timeout.

## Living documentation

Every session leaves knowledge behind, per project, in a `.agora/` folder inside the project directory:

```
.agora/
  KNOWLEDGE.md          rolling digest: overview, layout, conventions, decisions, gotchas, how to run & test
  KNOWLEDGE.prev.md     previous version, in case a merge went wrong
  notes/
    2026-09-06-1458-ada.md   one note per finished session: summary, changes, decisions, learnings, open threads
```

**How it happens.** When an agent exits, or you kill it, agora collects the session transcript and asks a headless model to write the session note and re-merge the digest. The transcript comes from the harness's own session file when available (Claude Code's JSONL, Codex's rollout, readable text pulled from Antigravity's conversation database, Gemini's chat file), else from a raw output log agora taps from the pane, else from tmux scrollback. Sessions that are too short to say anything are skipped.

**How it flows back.** Every new agent in that directory gets the digest, plus the titles of recent notes, appended to its system prompt. Agents can also add facts live with `agora_remember(text)`, and read everything with `agora_knowledge()`.

**Commands and keys.** `agora notes [dir]` prints a project's digest and note list. `agora document <name>` snapshots a running agent into a note without stopping it. In the sidebar, `o` opens the selected agent's project digest in `$EDITOR`. `agora kill <name> --no-docs` skips documentation.

**Summarizer.** Defaults to the first of `claude`, `codex`, `gemini`, `agy` found on PATH, run headless with no tools and a plain writer system prompt. Configure in `~/.local/share/agora/config.json`:

```json
{ "summarizer": "auto", "model": null, "document_sessions": true }
```

`summarizer` accepts `auto`, `claude`, `codex`, `gemini`, `agy`, or `none`. `model` is passed through to the CLI. Env overrides: `AGORA_SUMMARIZER`, `AGORA_SUMMARIZER_MODEL`. Commit `.agora/` if you want the knowledge shared with your team, or add it to `.gitignore` to keep it local.

## Status

**Beta.** Linux and macOS are the supported platforms; Windows runs via WSL2 but is unofficial until a native story exists. Tested against real agents: Claude Code and Antigravity end to end (messaging, waiting, nudges, documentation, debates). Codex and Gemini launch with the bus wired but have had less real-world time. `npm test` exercises the workspace, bus, status detection and MCP server with a fake harness, and CI runs it on Ubuntu and macOS.

Things to know before relying on it:

- **It edits two Antigravity config files** the first time you spawn an agy agent (MCP registration and `mcp(agora/*)` allow rules), and pre-approves its own tools in Claude Code with `--allowedTools mcp__agora`. It tells you when it does. Nothing else on your machine is touched outside `~/.local/share/agora` and `<project>/.agora/`.
- **Agents drive each other.** A message from one agent is typed into another's prompt. Content in a repository could, in principle, steer one agent into instructing another. Treat agents in agora like agents anywhere: give them the permissions you'd give a contractor, and read what they send you.
- **Spawning is capped.** Agents may start other agents, but only up to `max_agents` (default 8). Humans can always spawn more.
- **You pay for the tokens.** Every agent, every debate round, and every session summary runs on your own CLI subscriptions or API keys.

## Notes and limits

- **First run per project**: Claude Code and agy ask whether you trust the folder, Codex may ask you to sign in, Gemini may ask about tool permissions. agora flags these as `!` so you can answer them.
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
lib/knowledge.js living docs: session notes, KNOWLEDGE.md digest, summarizer
lib/debate.js    moderated multi-agent debates → .agora/decisions/
lib/doctor.js    `agora doctor` environment checks
test/            node:test suite; fixtures/claude is a fake harness so CI needs no real agent
install.sh       Linux / macOS / WSL installer
packaging/       AUR PKGBUILD and Homebrew formula templates
windows/         PowerShell + cmd launchers and installer (run agora inside WSL2)
lib/transcript.js transcript collection (Claude JSONL / Codex rollout / Gemini chat / raw log / scrollback)
lib/ui.js        sidebar TUI
lib/db.js        SQLite (node:sqlite)
```
