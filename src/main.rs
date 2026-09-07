mod client;
mod config;
mod ctl;
mod daemon;
mod db;
mod debate;
mod doctor;
mod harness;
mod knowledge;
mod mcp;
mod paths;
mod proto;
mod status;
mod theme;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

use proto::{ClientMsg, DaemonMsg, SpawnReq};

#[derive(Parser)]
#[command(name = "qoral", version, about = "Many coding agents, one terminal. They talk, debate and document.")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the workspace (starts the daemon if needed)
    Open {
        /// Agent to show first
        agent: Option<String>,
    },
    /// Run the daemon in the foreground (normally started automatically)
    Daemon,
    /// Start the daemon detached without attaching a client
    Up,
    /// Start an agent
    Spawn {
        /// claude | codex | agy | gemini
        harness: String,
        #[arg(long)]
        name: Option<String>,
        /// Working directory (default: current)
        #[arg(long, alias = "cwd")]
        dir: Option<String>,
        /// Don't switch attached clients to the new agent
        #[arg(long)]
        no_focus: bool,
        /// The task to give the agent
        #[arg(trailing_var_arg = true)]
        task: Vec<String>,
    },
    /// Message an agent ("all" broadcasts, "human" reaches the operator)
    Send {
        to: String,
        #[arg(long, default_value = "human")]
        from: String,
        #[arg(trailing_var_arg = true)]
        message: Vec<String>,
    },
    /// List agents and status
    #[command(alias = "ls", alias = "ps")]
    List,
    /// Recent bus traffic
    Log {
        #[arg(short = 'n', default_value_t = 50)]
        n: usize,
    },
    /// Show an agent in attached clients
    Focus { name: String },
    /// Print an agent's current screen as text (for scripts, tests and debugging)
    Screen { name: String },
    /// Type a line into an agent's terminal (text, then Enter), for scripts and tests
    Type {
        name: String,
        #[arg(trailing_var_arg = true)]
        text: Vec<String>,
    },
    /// Stop an agent (its session gets documented unless --no-docs)
    #[command(alias = "rm")]
    Kill {
        name: String,
        #[arg(long)]
        no_docs: bool,
    },
    /// Stop the daemon and every agent
    #[command(alias = "quit")]
    Stop,
    /// MCP server for an agent (stdio); used by the harness configs
    Mcp {
        #[arg(long)]
        agent: Option<String>,
    },
    /// Show a project's knowledge digest and session notes
    #[command(alias = "knowledge", alias = "docs")]
    Notes {
        dir: Option<String>,
        #[arg(short = 'n', default_value_t = 20)]
        n: usize,
    },
    /// Write a session note for a running agent now (it keeps running)
    #[command(alias = "doc")]
    Document { name: String },
    /// Check terminal, agent CLIs, daemon, data and print platform tips
    Doctor,
    /// List/preview themes, or scaffold a custom one: theme init <name> [--from nord]
    #[command(alias = "themes")]
    Theme {
        #[arg(default_value = "list")]
        action: String,
        name: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Several agents argue a question, then a decision document is written
    Debate {
        /// The question to debate (quote it)
        question: String,
        #[arg(long)]
        dir: Option<String>,
        /// Comma-separated harnesses for the participants (2-4), e.g. claude,agy,codex
        #[arg(long)]
        agents: Option<String>,
        #[arg(long, default_value_t = 3)]
        rounds: usize,
        /// Seconds to wait per round
        #[arg(long, default_value_t = 300)]
        timeout: u64,
        /// Spawn a builder afterwards to implement the decision
        #[arg(long)]
        build: bool,
        /// Leave participants running afterwards
        #[arg(long)]
        keep: bool,
        /// Options mode: each participant develops a distinct candidate; you get a comparison, then choose with `qoral build`
        #[arg(long)]
        options: bool,
    },
    /// Spawn a builder to implement a decision document, or one option of an options document
    Build {
        /// Path to the .qoral/decisions/*.md file
        file: String,
        /// Which option to implement (required for options documents)
        #[arg(long)]
        option: Option<usize>,
        /// Harness for the builder (default: claude if available)
        #[arg(long)]
        harness: Option<String>,
        /// Project directory (default: the project the document belongs to)
        #[arg(long)]
        dir: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // The MCP server is synchronous stdio; keep it off the async runtime entirely.
    if let Some(Cmd::Mcp { agent }) = &cli.cmd {
        let agent = agent.clone().or_else(|| std::env::var("QORAL_AGENT").ok());
        let Some(agent) = agent else { bail!("mcp: --agent <name> is required (or QORAL_AGENT)") };
        return mcp::serve(&agent);
    }
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async_main(cli))
}

async fn async_main(cli: Cli) -> Result<()> {
    match cli.cmd {
        None | Some(Cmd::Open { agent: None }) => client::run(None).await,
        Some(Cmd::Open { agent }) => client::run(agent).await,
        Some(Cmd::Daemon) => {
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
                .with_target(false)
                .init();
            daemon::run().await
        }
        Some(Cmd::Up) => {
            let _ = ctl::connect().await?;
            println!("daemon up. run `qoral` to attach.");
            Ok(())
        }
        Some(Cmd::Spawn { harness, name, dir, no_focus, task }) => {
            let prompt = task.join(" ").trim().to_string();
            let cwd = dir.unwrap_or_else(|| std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default());
            let req = SpawnReq { harness, name, cwd: Some(cwd), prompt: if prompt.is_empty() { None } else { Some(prompt) }, focus: !no_focus, command: None };
            match ctl::request(ClientMsg::Spawn(req), true).await? {
                DaemonMsg::Spawned { name, notices } => {
                    let db = db::Db::open()?;
                    let a = db.get_agent(&name)?;
                    println!("started {name} ({}) in {}", a.as_ref().map(|a| a.harness.as_str()).unwrap_or("?"), a.as_ref().map(|a| a.cwd.as_str()).unwrap_or("?"));
                    for n in notices {
                        eprintln!("note: {n}");
                    }
                    Ok(())
                }
                DaemonMsg::Error { message } => bail!("{message}"),
                other => bail!("unexpected reply: {other:?}"),
            }
        }
        Some(Cmd::Send { to, from, message }) => {
            let body = message.join(" ");
            if body.trim().is_empty() {
                bail!("usage: qoral send <agent|all|human> <message>");
            }
            let db = db::Db::open()?;
            let targets = db::send_message(&db, &from, &to, &body)?;
            println!("{}", if targets.is_empty() { "no recipients".to_string() } else { format!("queued for {}", targets.join(", ")) });
            Ok(())
        }
        Some(Cmd::List) => {
            let live = ctl::list_agents().await.unwrap_or_default();
            let db = db::Db::open()?;
            let rows = db.list_agents(false)?;
            if rows.is_empty() && live.is_empty() {
                println!("no agents");
                return Ok(());
            }
            for a in rows {
                let status = live.iter().find(|l| l.name == a.name).map(|l| l.status.clone()).unwrap_or(a.status.clone());
                let pending = db.count_pending(&a.name).unwrap_or(0);
                println!(
                    "{:<14} {:<7} {:<11} {}{}{}",
                    a.name,
                    a.harness,
                    status,
                    if pending > 0 { format!("↓{pending} ") } else { String::new() },
                    a.cwd,
                    a.task.as_deref().map(|t| format!("  — {t}")).unwrap_or_default()
                );
            }
            let h = db.count_unread("human")?;
            if h > 0 {
                println!("\n{h} unread message(s) for you — see `qoral log`");
            }
            Ok(())
        }
        Some(Cmd::Log { n }) => {
            let db = db::Db::open()?;
            let msgs = db.recent_messages(n)?;
            let ids: Vec<i64> = msgs.iter().filter(|m| m.recipient == "human" && m.read_at.is_none()).map(|m| m.id).collect();
            db.mark_read(&ids)?;
            if msgs.is_empty() {
                println!("no messages yet");
            }
            for m in msgs {
                let t = chrono::DateTime::from_timestamp_millis(m.ts).map(|d| d.with_timezone(&chrono::Local).format("%H:%M:%S").to_string()).unwrap_or_default();
                println!("{t} {} → {}: {}", m.sender, m.recipient, m.body);
            }
            Ok(())
        }
        Some(Cmd::Screen { name }) => match ctl::request(ClientMsg::Screen { name }, false).await? {
            DaemonMsg::Text { text } => {
                print!("{text}");
                Ok(())
            }
            DaemonMsg::Error { message } => bail!("{message}"),
            other => bail!("unexpected reply: {other:?}"),
        },
        Some(Cmd::Type { name, text }) => {
            let line = text.join(" ");
            let mut stream = match ctl::try_connect().await {
                Some(s) => s,
                None => bail!("qoral daemon is not running"),
            };
            proto::write_msg(&mut stream, &ClientMsg::Hello { client: "cli".into() }).await?;
            proto::write_msg(&mut stream, &ClientMsg::Input { agent: name.clone(), data: line.into_bytes() }).await?;
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            proto::write_msg(&mut stream, &ClientMsg::Input { agent: name, data: b"\r".to_vec() }).await?;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            Ok(())
        }
        Some(Cmd::Focus { name }) => match ctl::request(ClientMsg::Focus { name }, false).await? {
            DaemonMsg::Ok => Ok(()),
            DaemonMsg::Error { message } => bail!("{message}"),
            other => bail!("unexpected reply: {other:?}"),
        },
        Some(Cmd::Kill { name, no_docs }) => match ctl::request(ClientMsg::Kill { name: name.clone() }, false).await {
            Ok(DaemonMsg::Ok) => {
                println!("killed {name}{}", if no_docs { "" } else { " (documenting its session in the background if it ran long enough)" });
                Ok(())
            }
            Ok(DaemonMsg::Error { message }) => bail!("{message}"),
            Ok(other) => bail!("unexpected reply: {other:?}"),
            Err(_) => {
                // daemon down: clean the row
                db::Db::open()?.delete_agent(&name)?;
                println!("removed {name} (daemon not running)");
                Ok(())
            }
        },
        Some(Cmd::Stop) => match ctl::request(ClientMsg::Shutdown, false).await {
            Ok(_) => {
                println!("qoral stopped");
                Ok(())
            }
            Err(_) => {
                println!("qoral daemon was not running");
                Ok(())
            }
        },
        Some(Cmd::Notes { dir, n }) => {
            let cwd = dir.map(|d| paths::expand_home(&d)).unwrap_or(std::env::current_dir()?);
            let know = knowledge::read_knowledge(&cwd);
            if know.trim().is_empty() {
                println!("no knowledge recorded yet in {}/.qoral/", cwd.display());
            } else {
                println!("{}", know.trim());
            }
            let notes = knowledge::list_notes(&cwd, n);
            if !notes.is_empty() {
                println!("\nsession notes:");
                for (f, t) in notes {
                    println!("  {f}  {t}");
                }
            }
            let decisions = cwd.join(".qoral/decisions");
            if let Ok(rd) = std::fs::read_dir(&decisions) {
                let mut ds: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|f| f.ends_with(".md")).collect();
                ds.sort();
                if !ds.is_empty() {
                    println!("\ndecisions:");
                    for d in ds {
                        println!("  .qoral/decisions/{d}");
                    }
                }
            }
            Ok(())
        }
        Some(Cmd::Document { name }) => match ctl::request(ClientMsg::Document { name: name.clone() }, false).await? {
            DaemonMsg::Ok => {
                println!("documenting {name} in the background; the note lands in its project's .qoral/notes and a summary arrives on the bus (qoral log)");
                Ok(())
            }
            DaemonMsg::Error { message } => bail!("{message}"),
            other => bail!("unexpected reply: {other:?}"),
        },
        Some(Cmd::Doctor) => {
            let problems = doctor::run().await;
            std::process::exit(if problems > 0 { 1 } else { 0 });
        }
        Some(Cmd::Theme { action, name, from, force }) => match action.as_str() {
            "list" | "show" => {
                println!("{}", theme::describe());
                println!("\nset one with:  \"theme\": \"<name>\" in {}/config.json  (or QORAL_THEME=<name> qoral)", paths::home().display());
                Ok(())
            }
            "init" | "new" => {
                let file = theme::init_theme(name.as_deref().unwrap_or("custom"), from.as_deref(), force)?;
                let stem = file.file_stem().unwrap().to_string_lossy();
                println!("wrote {}\nedit it, then set \"theme\": \"{stem}\" in {}/config.json (or run: QORAL_THEME={stem} qoral)", file.display(), paths::home().display());
                Ok(())
            }
            other => bail!("unknown theme action \"{other}\" (list | init <name> [--from <builtin>] [--force])"),
        },
        Some(Cmd::Debate { question, dir, agents, rounds, timeout, build, keep, options }) => {
            let opts = debate::DebateOpts {
                question,
                cwd: dir.map(|d| paths::expand_home(&d)).unwrap_or(std::env::current_dir()?),
                harnesses: agents.map(|a| a.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default(),
                rounds: rounds.max(1),
                timeout: std::time::Duration::from_secs(timeout.max(10)),
                build,
                keep,
                options,
            };
            debate::run(opts, &|m| println!("{m}")).await
        }
        Some(Cmd::Build { file, option, harness, dir }) => {
            let name = debate::build(&paths::expand_home(&file), option, harness, dir.map(|d| paths::expand_home(&d))).await?;
            println!("builder {name} started; it reports to you on the bus when done (qoral log)");
            Ok(())
        }
        Some(Cmd::Mcp { .. }) => unreachable!(),
    }
}
