# qoral

Many coding agents, one terminal. They talk to each other, argue out decisions, and every session leaves documentation behind.

qoral is a terminal workspace for running several AI coding agents side by side: Claude Code, OpenAI Codex, Google Antigravity CLI (`agy`), and Gemini CLI, in any mix, across any projects. A sidebar lists every agent with live status; the main pane shows whichever agent you select, rendered by a real terminal emulator. Every agent gets a `qoral` MCP server so agents can message each other, broadcast, wait for replies, delegate by spawning sub-agents, and message you. When a session ends it is summarized into a per-project knowledge base that every future agent in that project receives. And when you have a question, three agents can debate it and hand you a decision document.

One Rust binary. No tmux, no Node, no runtime dependencies beyond the agent CLIs you already use.

```
┌─ qoral ◂─────────────┬──────────────────────────────────────────────────┐
│ ▸1 ● ada     claude  │  ❯ [qoral] message from grace: the API tests     │
│  2 ◐ grace   agy     │    fail on /users; can you check the model?      │
│  3 ! linus   codex   │  ● Looking at models/user.js ...                 │
│                      │                                                  │
│ ~/Projects/api       │                                                  │
│ fix the failing…     │                                                  │
│ ── bus ───────────── │                                                  │
│ grace→ada the API…   │                                                  │
│ •ada→human done, PR… │                                                  │
│ n new D debate m msg │                                                  │
└──────────────────────┴──────────────────────────────────────────────────┘
 qoral  M-n new  M-m message  M-←/→ focus  M-[ M-] prev/next  M-1..9 jump  M-d detach
```

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/shreeyanshujha/qoral/main/install.sh | sh
qoral doctor
```

The installer picks the prebuilt binary for your platform (Linux x86_64/aarch64, macOS Intel/Apple silicon) and falls back to building with cargo. Alternatives:

```sh
cargo install --git https://github.com/shreeyanshujha/qoral qoral   # from source
git clone https://github.com/shreeyanshujha/qoral && cd qoral && ./install.sh --build
```

Packaging templates for the AUR and Homebrew live in `packaging/`. Windows: WSL2 works today (see `windows/`); native ConPTY support is in progress.

## Use

```sh
qoral                              # open the workspace (starts the daemon if needed)
qoral spawn claude --dir ~/proj "write tests for lib/parse.js"
qoral spawn agy    --dir ~/proj --name grace "review ada's tests when she pings you"
qoral send ada "grace will review; ping her when done"
qoral send all "wrap up in 10 minutes"
qoral list                         # agents + status
qoral log                          # bus traffic
qoral debate "how should divide() handle division by zero?" --dir ~/proj --build
qoral notes ~/proj                 # the project's knowledge digest, notes and decisions
qoral kill grace                   # its session gets documented
qoral stop                         # tear everything down
```

The daemon keeps running when you close the terminal; `qoral` reattaches. Several terminals can attach at once.

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
| `o` | open the project's `.qoral/KNOWLEDGE.md` in `$EDITOR` |
| `?` | help |
| `d` | detach; agents keep running, `qoral` brings you back |
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

Mouse: click to focus and select, wheel scrolls an agent's history unless the agent asked for the mouse itself. Paste is bracketed when the agent supports it.

Status glyphs: `●` idle at prompt · `◐` working · `!` needs you (permission, trust or login dialog) · `✎` documenting · `○` exited.

## Supported harnesses

| harness | command | MCP wiring | identity | permission pre-approval |
|---|---|---|---|---|
| `claude` | Claude Code | `--mcp-config` per agent | `--agent` arg | `--allowedTools mcp__qoral` |
| `codex` | OpenAI Codex | `-c mcp_servers.qoral...` per agent | `--agent` arg | follows your Codex approval policy |
| `agy` | Google Antigravity CLI | global `~/.gemini/config/mcp_config.json`, merged idempotently | `QORAL_AGENT` env (agy passes env through) | `mcp(qoral/<tool>)` rules merged into `~/.gemini/antigravity-cli/settings.json` |
| `gemini` | Gemini CLI | per-agent system-settings file via `GEMINI_CLI_SYSTEM_SETTINGS_PATH` | `--agent` arg | `trust: true` on the server |

agy has no per-session MCP flag and no system-prompt flag, so qoral registers the server once in your agy config (and pre-approves its tools) and sends the agent's briefing as the initial prompt. Both edits are additive and idempotent, and qoral tells you when it makes them.

## How agents talk

Each agent is launched with a `qoral` MCP server. Tools:

| tool | purpose |
|---|---|
| `qoral_list_agents` | who is running, their status and task |
| `qoral_send(to, message)` | message an agent, `all`, or `human` |
| `qoral_inbox()` | unread messages |
| `qoral_wait(timeout_seconds, from?)` | block until a reply arrives |
| `qoral_spawn(harness, name, cwd, prompt)` | delegate to a new agent |
| `qoral_log(limit)` | recent bus traffic |
| `qoral_remember(text)` | save a durable project fact for future agents |
| `qoral_knowledge()` | read the project digest and note list |

Messages live in SQLite at `~/.local/share/qoral/qoral.db`. The daemon delivers them: when a recipient is idle at its prompt, the pending message is typed into it as `[qoral] message from X: …`, so the agent reacts without polling. Busy agents get it when they next go idle, or immediately if they call `qoral_inbox` / `qoral_wait`. Messages to `human` show up in the sidebar bus and in `qoral log`.

Every agent's briefing explains its name, who else is in the room, and the etiquette: be self-contained, coordinate before touching shared files, report back when delegated work is done.

## Debates: let agents argue it out

```sh
qoral debate "Should we move the tests to node:test, and how?" --dir ~/proj --build
```

qoral spawns three participants with different perspectives (a pragmatist, a skeptic, a minimalist; an architect if you ask for four) on different harnesses, and moderates them itself over the bus. The moderator is a deterministic process, not another model, so the protocol is enforced:

1. Each participant reads the code, then sends an opening position to `moderator` ending in `STANCE:` and `CONSENSUS:` lines.
2. Each round the moderator relays everyone's positions to everyone else. Participants critique, concede or hold, and reply.
3. It stops when every participant writes `CONSENSUS: yes`, or after the round limit.
4. A headless model writes a decision document to `.qoral/decisions/<date>-<slug>.md`: decision or recommendation, rationale, alternatives considered, dissent and risks, a numbered implementation plan, and the full transcript. A one-line link is added under `## Decisions` in `KNOWLEDGE.md`, so every future agent inherits the outcome.
5. With `--build`, a builder agent implements the plan, runs the tests, and reports to you.

Flags: `--agents claude,agy,codex` (2 to 4, repeats allowed), `--rounds 3`, `--timeout 300` seconds per round, `--keep` to leave participants running. From the sidebar, `D` runs a debate in its own window so you can watch the moderator's log. Participants stuck on a permission prompt are flagged in the log and on the bus.

### Options mode: a menu instead of a verdict

```sh
qoral debate "How should we add caching to the API?" --dir ~/proj --options --rounds 2
```

With `--options`, participants don't try to agree. Each develops a distinct candidate from its perspective, stress-tests the others' candidates for a round, and the write-up is a comparison: per option its approach, effort, gains, costs and risks, the objections raised and whether they were answered, and "choose this if"; then a comparison table and a clearly labelled optional recommendation. It lands in `.qoral/decisions/<date>-options-<slug>.md`, is linked from `KNOWLEDGE.md` as undecided, and arrives on your bus as `OPTIONS: …`. You choose:

```sh
qoral build .qoral/decisions/2026-09-07-1010-options-how-should-we-add-caching.md --option 2
```

`qoral build` spawns a builder for that option (or for a decision document without `--option`), which implements it, runs the tests and reports to you. In the sidebar, `D` asks whether you want a decision or options.

## Living documentation

Every session leaves knowledge behind, per project, in a `.qoral/` folder inside the project directory:

```
.qoral/
  KNOWLEDGE.md          rolling digest: overview, layout, conventions, decisions, gotchas, how to run & test
  KNOWLEDGE.prev.md     previous version, in case a merge went wrong
  notes/                one note per finished session: summary, changes, decisions, learnings, open threads
  decisions/            one document per debate
```

**How it happens.** When an agent exits, or you kill it, the daemon collects the session transcript and asks a headless model to write the session note and re-merge the digest. The transcript comes from the harness's own session file when available (Claude Code's JSONL, Codex's rollout, readable text pulled from Antigravity's conversation database, Gemini's chat file), else from the raw PTY log qoral keeps, else from the emulator's scrollback. Sessions too short to say anything are skipped. `qoral document <name>` snapshots a running agent without stopping it.

**How it flows back.** Every new agent in that directory gets the digest, plus the titles of recent notes, in its briefing. Agents add facts live with `qoral_remember(text)` and read everything with `qoral_knowledge()`.

**Summarizer.** Defaults to the first of `claude`, `codex`, `gemini`, `agy` on PATH, run headless with no tools and a plain writer system prompt. Configure in `~/.local/share/qoral/config.json`:

```json
{ "summarizer": "auto", "model": null, "document_sessions": true, "max_agents": 8, "theme": "default" }
```

`summarizer` accepts `auto`, `claude`, `codex`, `gemini`, `agy`, or `none`. Env overrides: `QORAL_SUMMARIZER`, `QORAL_SUMMARIZER_MODEL`. Commit `.qoral/` to share the knowledge with your team, or add it to `.gitignore` to keep it local.

## Theming

```sh
qoral theme                    # list themes and preview the active palette
qoral theme init mine --from nord   # writes ~/.local/share/qoral/themes/mine.json
```

Built-in: `default`, `mono`, `nord`, `gruvbox`, `dracula`. Set `"theme": "gruvbox"` in the config, or `QORAL_THEME=nord qoral` for one run. A theme file has `colors` and `glyphs`; anything omitted inherits from `default`. Colors accept a name (`"red"`, `"brightblue"`), a 256-color index (`214`), or hex (`"#fabd2f"`, truecolor). Single roles can be overridden inline:

```json
{ "theme": "nord", "colors": { "accent": "#ff6ac1" }, "glyphs": { "idle": "•", "working": ["⠋","⠙","⠹","⠸"] } }
```

Roles: `accent`, `idle`, `working`, `attention`, `documenting`, `moderating`, `exited`, `text`, `dim`, `border`, `mail`, per-harness `claude`/`codex`/`agy`/`gemini`/`moderator`, and the bar's `bar_fg`/`bar_bg`/`bar_key`.

## How it works

```
 terminal ── qoral (client, ratatui) ──┐
 terminal ── qoral (client)  ──────────┼── unix socket ── qoral daemon
                                       │                    ├─ agent "ada":   PTY ⇄ terminal emulator (alacritty_terminal), 20k lines history
 qoral spawn / send / debate (CLI) ────┘                    ├─ agent "grace": PTY ⇄ emulator
                                                            ├─ status detection from the screen grid, every 400 ms
                                                            ├─ bus delivery: type pending messages into idle agents
                                                            └─ documentation on exit/kill (headless summarizer, off-thread)
 agents ── qoral mcp (stdio JSON-RPC) ── SQLite bus (~/.local/share/qoral/qoral.db)
```

The daemon starts on first use and lives in the background; the socket sits in `$XDG_RUNTIME_DIR/qoral/`. Frames are sent as MessagePack at up to 60 fps only for the agent a client is looking at. `QORAL_HOME` relocates the data directory (and gives that workspace its own daemon), which is how the tests run.

## Platforms

| platform | status | notes |
|---|---|---|
| **Linux** | supported | Any terminal (Foot, Alacritty, Kitty, GNOME Terminal, Konsole, xterm). |
| **macOS** | supported | Alt chords need Option to send Meta: Terminal.app → Settings → Profiles → Keyboard → *Use Option as Meta key*; iTerm2 → Profiles → Keys → *Left Option key: Esc+*. |
| **Windows** | WSL2 today, native in progress | The PTY layer (portable-pty) already supports ConPTY; the socket transport and process handling are the remaining native pieces. `windows\install.ps1` sets up the WSL2 path. |

## Status

**Beta.** Tested against real agents: Claude Code and Antigravity end to end (messaging, waiting, nudges, documentation, debates). Codex and Gemini launch with the bus wired but have had less real-world time. `cargo test` drives a real daemon with a fake harness, and CI runs it on Ubuntu and macOS.

Things to know before relying on it:

- **It edits two Antigravity config files** the first time you spawn an agy agent (MCP registration and `mcp(qoral/*)` allow rules), and pre-approves its own tools in Claude Code with `--allowedTools mcp__qoral`. It tells you when it does. Nothing else outside `~/.local/share/qoral` and `<project>/.qoral/` is touched.
- **Agents drive each other.** A message from one agent is typed into another's prompt. Content in a repository could, in principle, steer one agent into instructing another. Give agents the permissions you'd give a contractor, and read what they send you.
- **Spawning is capped.** Agents may start other agents up to `max_agents` (default 8). Humans can always spawn more.
- **You pay for the tokens.** Every agent, every debate round, and every session summary runs on your own CLI subscriptions or API keys.
- **Status is heuristic.** It reads the last screenful of each agent. Unusual prompts may show as "working" briefly.

## Layout

See `CONTRIBUTING.md` for the source layout and how to add a harness.

## License

MIT.
