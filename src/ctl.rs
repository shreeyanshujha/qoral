//! Client-side helpers to reach the daemon: connect, auto-start, one-shot requests.
use anyhow::{bail, Context, Result};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::net::UnixStream;

use crate::paths;
use crate::proto::{read_msg, write_msg, ClientMsg, DaemonMsg};

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

/// Connect, starting the daemon if needed.
pub async fn connect() -> Result<UnixStream> {
    if let Some(s) = try_connect().await {
        return Ok(s);
    }
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

pub async fn list_agents() -> Result<Vec<crate::proto::AgentInfo>> {
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
