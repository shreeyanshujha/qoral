//! Client-side helpers to reach the daemon: connect, auto-start, one-shot requests.
use anyhow::{bail, Context, Result};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::net::UnixStream;

use crate::paths;
use crate::proto::{build_id, read_msg, write_msg, AgentInfo, ClientMsg, DaemonMsg};

pub async fn try_connect() -> Option<UnixStream> {
    UnixStream::connect(paths::socket_path()).await.ok()
}

pub fn start_daemon_detached() -> Result<()> {
    paths::ensure_dirs()?;
    let exe = std::env::current_exe()?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(paths::daemon_log())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon").stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().context("start daemon")?;
    Ok(())
}

async fn start_and_wait() -> Result<UnixStream> {
    let _ = std::fs::remove_file(paths::socket_path());
    start_daemon_detached()?;
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Some(s) = try_connect().await {
            return Ok(s);
        }
    }
    bail!("daemon did not come up; see {}", paths::daemon_log().display())
}

/// Connect, starting the daemon if needed. If a daemon from a different build of qoral is running
/// (after an upgrade or rebuild) and it has no live agents, it is restarted so the two sides agree
/// on the protocol. The returned stream has its Hello consumed; the initial Agents list follows.
pub async fn connect() -> Result<UnixStream> {
    let mut stream = match try_connect().await {
        Some(s) => s,
        None => return start_and_wait().await,
    };
    let hello: DaemonMsg = match tokio::time::timeout(Duration::from_secs(3), read_msg(&mut stream)).await {
        Ok(Ok(m)) => m,
        _ => {
            // unresponsive or unreadable daemon: replace it
            drop(stream);
            return start_and_wait().await;
        }
    };
    let daemon_build = match &hello {
        DaemonMsg::Hello { build, .. } => build.clone(),
        _ => String::new(),
    };
    if daemon_build == build_id() {
        return Ok(stream);
    }
    // different build: peek at the agent list; restart only if nothing is running
    let agents: Vec<AgentInfo> = match tokio::time::timeout(Duration::from_secs(3), read_msg::<_, DaemonMsg>(&mut stream)).await {
        Ok(Ok(DaemonMsg::Agents(v))) => v,
        _ => Vec::new(),
    };
    let live = agents.iter().filter(|a| a.exited_at.is_none() && a.harness != "moderator").count();
    if live == 0 {
        let _ = write_msg(&mut stream, &ClientMsg::Shutdown).await;
        drop(stream);
        // give it a moment to exit and release the socket
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if try_connect().await.is_none() {
                break;
            }
        }
        return start_and_wait().await;
    }
    // agents are running under the old daemon: keep it, but re-request the list we consumed
    eprintln!("note: the qoral daemon is an older build; it will be replaced when no agents are running (or: qoral stop)");
    write_msg(&mut stream, &ClientMsg::ListAgents).await?;
    Ok(stream)
}

/// Send one request and return the first reply that isn't a routine broadcast.
pub async fn request(msg: ClientMsg, auto_start: bool) -> Result<DaemonMsg> {
    let mut stream = if auto_start {
        connect().await?
    } else {
        match try_connect().await {
            Some(s) => s,
            None => bail!("qoral daemon is not running"),
        }
    };
    write_msg(&mut stream, &ClientMsg::Hello { client: "cli".into() }).await?;
    write_msg(&mut stream, &msg).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let reply: DaemonMsg = tokio::time::timeout_at(deadline, read_msg(&mut stream)).await.context("daemon reply timeout")??;
        match reply {
            DaemonMsg::Hello { .. } | DaemonMsg::Agents(_) | DaemonMsg::Frame(_) | DaemonMsg::Focused { .. } => continue,
            other => return Ok(other),
        }
    }
}

pub async fn list_agents() -> Result<Vec<AgentInfo>> {
    let mut stream = match try_connect().await {
        Some(s) => s,
        None => return Ok(Vec::new()),
    };
    write_msg(&mut stream, &ClientMsg::Hello { client: "cli".into() }).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let reply: DaemonMsg = tokio::time::timeout_at(deadline, read_msg(&mut stream)).await??;
        if let DaemonMsg::Agents(v) = reply {
            return Ok(v);
        }
    }
}
