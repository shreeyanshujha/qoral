mod client;
mod config;
mod ctl;
mod daemon;
mod db;
mod harness;
mod mcp;
mod paths;
mod proto;
mod status;

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
    /// Stop an agent
    #[command(alias = "rm")]
    Kill { name: String },
    /// Stop the daemon and every agent
    #[command(alias = "quit")]
    Stop,
    /// MCP server for an agent (stdio); used by the harness configs
    Mcp {
        #[arg(long)]
        agent: Option<String>,
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
            let req = SpawnReq { harness, name, cwd: Some(cwd), prompt: if prompt.is_empty() { None } else { Some(prompt) }, focus: !no_focus };
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
        Some(Cmd::Focus { name }) => match ctl::request(ClientMsg::Focus { name }, false).await? {
            DaemonMsg::Ok => Ok(()),
            DaemonMsg::Error { message } => bail!("{message}"),
            other => bail!("unexpected reply: {other:?}"),
        },
        Some(Cmd::Kill { name }) => match ctl::request(ClientMsg::Kill { name: name.clone() }, false).await {
            Ok(DaemonMsg::Ok) => {
                println!("killed {name}");
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
        Some(Cmd::Mcp { .. }) => unreachable!(),
    }
}
