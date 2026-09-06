//! The daemon: owns agents (PTY + emulator), serves clients over a Unix socket,
//! detects status, delivers bus messages into idle agents.
pub mod agent;

use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::db::{self, Db};
use crate::harness;
use crate::knowledge::{self, DocOutcome, SessionInfo};
use crate::paths;
use crate::proto::{AgentInfo, ClientMsg, DaemonMsg, SpawnReq};
use crate::status::detect_status;
use agent::Agent;

/// Everything the documentation thread needs, owned (the agent may be gone by the time it runs).
pub struct DocJob {
    pub name: String,
    pub harness: String,
    pub cwd: PathBuf,
    pub task: Option<String>,
    pub started_at: i64,
    pub ended_at: i64,
    pub session_id: Option<String>,
    pub raw_log: PathBuf,
    pub scrollback: String,
}

impl DocJob {
    fn from_agent(a: &Agent) -> DocJob {
        DocJob {
            name: a.name.clone(),
            harness: a.harness.clone(),
            cwd: a.cwd.clone(),
            task: a.task.clone(),
            started_at: a.created_at,
            ended_at: db::now_ms(),
            session_id: a.launch.session_id.clone(),
            raw_log: paths::agent_dir(&a.name).join("raw.log"),
            scrollback: a.history_text(),
        }
    }
}

fn documentable(a: &Agent) -> bool {
    a.harness != "cmd" && knowledge::pick_summarizer().is_some() && crate::config::load().document_sessions
}

/// Run documentation off the runtime. When `shared` is given the agent still exists and gets a
/// banner + status update; otherwise the outcome is reported on the bus only.
fn document_in_background(job: DocJob, shared: Option<Shared>) {
    std::thread::Builder::new()
        .name(format!("doc-{}", job.name))
        .spawn(move || {
            let mut notes: Vec<String> = Vec::new();
            let outcome = {
                let info = SessionInfo {
                    name: &job.name,
                    harness: &job.harness,
                    cwd: &job.cwd,
                    task: job.task.as_deref(),
                    started_at: job.started_at,
                    ended_at: job.ended_at,
                    session_id: job.session_id.as_deref(),
                    raw_log: Some(&job.raw_log),
                    scrollback: Some(&job.scrollback),
                };
                knowledge::document_session(&info, &mut |m| notes.push(m.to_string()))
            };
            let summary = match &outcome {
                Ok(DocOutcome::Written { note, knowledge: k, summarizer, .. }) => {
                    format!("documented session of {} with {summarizer}: {}{}", job.name, note.display(), k.as_ref().map(|k| format!(" · knowledge: {}", k.display())).unwrap_or_default())
                }
                Ok(DocOutcome::Skipped(r)) => format!("session of {} not documented: {r}", job.name),
                Err(e) => format!("documentation of {} failed: {e}", job.name),
            };
            info!("{summary}");
            if let Ok(db) = Db::open() {
                if !matches!(outcome, Ok(DocOutcome::Skipped(_))) {
                    let _ = db.add_message("qoral", "human", &summary);
                }
                if shared.is_none() {
                    return;
                }
            }
            if let Some(shared) = shared {
                let mut s = shared.lock().unwrap();
                if let Some(a) = s.agents.get_mut(&job.name) {
                    for n in &notes {
                        a.banner(&format!("[qoral] {n}"));
                    }
                    a.banner(&format!("[qoral] {summary}"));
                    a.banner(&format!("[qoral] agent \"{}\" exited. Press x in the sidebar to remove it.", job.name));
                    a.status = "exited".into();
                }
                let _ = s.db.mark_exited(&job.name);
                s.publish_agents(true);
            }
        })
        .ok();
}

const IDLE_GRACE: Duration = Duration::from_millis(1500);
const MAX_NUDGE_CHARS: usize = 1800;
const DEFAULT_COLS: u16 = 160;
const DEFAULT_ROWS: u16 = 45;

pub struct ClientHandle {
    pub tx: mpsc::UnboundedSender<DaemonMsg>,
    pub subscribed: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub needs_frame: bool,
}

pub struct State {
    pub agents: HashMap<String, Agent>,
    pub clients: HashMap<u64, ClientHandle>,
    pub db: Db,
    next_client: u64,
    idle_since: HashMap<String, Instant>,
    last_agents: Vec<AgentInfo>,
    bytes_tx: mpsc::UnboundedSender<(String, Vec<u8>)>,
    pub shutting_down: bool,
}

pub type Shared = Arc<Mutex<State>>;

impl State {
    fn add_client(&mut self, tx: mpsc::UnboundedSender<DaemonMsg>) -> u64 {
        let id = self.next_client;
        self.next_client += 1;
        self.clients.insert(id, ClientHandle { tx, subscribed: None, cols: DEFAULT_COLS, rows: DEFAULT_ROWS, needs_frame: false });
        id
    }

    fn broadcast(&self, msg: DaemonMsg) {
        for c in self.clients.values() {
            let _ = c.tx.send(msg.clone());
        }
    }

    pub fn agent_infos(&self) -> Vec<AgentInfo> {
        let mut v: Vec<AgentInfo> = self
            .agents
            .values()
            .map(|a| a.info(self.db.count_pending(&a.name).unwrap_or(0)))
            .collect();
        // moderator rows (virtual, from the debate feature) live only in the db
        if let Ok(rows) = self.db.list_agents(true) {
            for r in rows {
                if r.harness == "moderator" && !self.agents.contains_key(&r.name) {
                    v.push(AgentInfo {
                        name: r.name,
                        harness: r.harness,
                        cwd: r.cwd,
                        status: r.status,
                        task: r.task,
                        created_at: r.created_at,
                        exited_at: None,
                        pending: 0,
                    });
                }
            }
        }
        v.sort_by_key(|a| a.created_at);
        v
    }

    fn publish_agents(&mut self, force: bool) {
        let infos = self.agent_infos();
        let changed = force
            || infos.len() != self.last_agents.len()
            || infos.iter().zip(self.last_agents.iter()).any(|(a, b)| {
                a.name != b.name || a.status != b.status || a.pending != b.pending || a.exited_at != b.exited_at
            });
        if changed {
            self.last_agents = infos.clone();
            self.broadcast(DaemonMsg::Agents(infos));
        }
    }

    pub fn spawn_agent(&mut self, req: &SpawnReq, size: Option<(u16, u16)>) -> Result<(String, Vec<String>)> {
        let is_cmd = req.command.is_some();
        if !is_cmd && !paths::HARNESSES.contains(&req.harness.as_str()) {
            bail!("unknown harness \"{}\" ({})", req.harness, paths::HARNESSES.join(" | "));
        }
        let taken: Vec<String> = self.agents.keys().cloned().collect();
        let name = req
            .name
            .as_deref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| harness::suggest_name(&taken));
        if !harness::valid_name(&name) {
            bail!("bad agent name \"{name}\" (letters, digits, - and _ only, max 32; not a reserved word)");
        }
        if let Some(existing) = self.agents.get(&name) {
            if existing.exited_at.is_none() {
                bail!("an agent named \"{name}\" is already running");
            }
            self.agents.remove(&name);
        }
        let cwd: PathBuf = match &req.cwd {
            Some(c) if !c.trim().is_empty() => paths::expand_home(c.trim()),
            _ => dirs::home_dir().unwrap_or_default(),
        };
        let cwd = if cwd.is_absolute() { cwd } else { std::env::current_dir()?.join(cwd) };
        if !cwd.is_dir() {
            bail!("directory not found: {}", cwd.display());
        }
        // stale db row from a previous daemon life
        let _ = self.db.delete_agent(&name);

        let (cols, rows) = size.unwrap_or((DEFAULT_COLS, DEFAULT_ROWS));
        let harness_name = if is_cmd { "cmd".to_string() } else { req.harness.clone() };
        let launch = match &req.command {
            Some(argv) if !argv.is_empty() => harness::command_launch(argv.clone()),
            Some(_) => bail!("empty command"),
            None => harness::build_launch(&req.harness, &name, &cwd, req.prompt.as_deref())?,
        };
        let raw_log = if is_cmd { None } else { Some(paths::agent_dir(&name).join("raw.log")) };
        let mut agent = Agent::spawn(&name, &harness_name, &cwd, req.prompt.as_deref(), launch, cols, rows, self.bytes_tx.clone(), raw_log)?;
        if is_cmd {
            agent.status = "shell".into();
        }
        let notices = agent.launch.notices.clone();
        let pid = agent.pid().map(|p| format!("pty:{p}"));
        self.db.insert_agent(&name, &harness_name, &cwd.display().to_string(), pid.as_deref(), agent.task.as_deref())?;
        // persist launch spec for documentation / debugging
        let _ = std::fs::write(
            paths::agent_dir(&name).join("launch.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "argv": agent.launch.argv, "env": agent.launch.env, "harness": req.harness, "name": name,
                "cwd": cwd, "prompt": req.prompt, "startedAt": agent.created_at, "sessionId": agent.launch.session_id,
            }))
            .unwrap_or_default(),
        );
        info!(agent = %name, harness = %harness_name, "spawned");
        self.agents.insert(name.clone(), agent);
        self.publish_agents(true);
        Ok((name, notices))
    }

    pub fn kill_agent(&mut self, name: &str, document: bool) -> Result<bool> {
        let Some(mut a) = self.agents.remove(name) else {
            if self.db.get_agent(name)?.is_some() {
                self.db.delete_agent(name)?;
                self.publish_agents(true);
                return Ok(false);
            }
            bail!("no agent named \"{name}\"");
        };
        let mut documenting = false;
        if document && a.exited_at.is_none() && documentable(&a) {
            document_in_background(DocJob::from_agent(&a), None);
            documenting = true;
        }
        a.kill();
        self.db.delete_agent(name)?;
        for c in self.clients.values_mut() {
            if c.subscribed.as_deref() == Some(name) {
                c.subscribed = None;
                c.needs_frame = true;
            }
        }
        self.idle_since.remove(name);
        info!(agent = %name, documenting, "killed");
        self.publish_agents(true);
        Ok(documenting)
    }

    /// Snapshot a running agent into a session note (it keeps running).
    pub fn document_agent(&self, name: &str) -> Result<()> {
        let Some(a) = self.agents.get(name) else { bail!("no agent named \"{name}\"") };
        if a.harness == "cmd" {
            bail!("\"{name}\" is a command window, nothing to document");
        }
        if knowledge::pick_summarizer().is_none() {
            bail!("no summarizer CLI available (claude/codex/gemini/agy) or summarizer=none");
        }
        document_in_background(DocJob::from_agent(a), None);
        Ok(())
    }

    /// Periodic housekeeping: exits, status, nudges.
    fn tick(&mut self, shared: &Shared) {
        let now = Instant::now();
        let names: Vec<String> = self.agents.keys().cloned().collect();
        for name in names {
            let Some(a) = self.agents.get_mut(&name) else { continue };
            if a.status == "documenting" {
                continue; // the doc thread will flip it to exited
            }
            if a.exited_at.is_none() {
                if let Some(code) = a.poll_exit() {
                    a.banner(&format!("[qoral] agent \"{name}\" exited ({code})."));
                    self.idle_since.remove(&name);
                    if documentable(a) {
                        a.status = "documenting".into();
                        a.banner(&format!("[qoral] documenting session… (writes .qoral/notes and .qoral/KNOWLEDGE.md in {})", a.cwd.display()));
                        let _ = self.db.set_status(&name, "documenting");
                        document_in_background(DocJob::from_agent(a), Some(shared.clone()));
                    } else {
                        a.banner("[qoral] Press x in the sidebar to remove it.");
                        let _ = self.db.mark_exited(&name);
                    }
                    continue;
                }
            } else {
                continue;
            }
            if a.harness == "cmd" {
                continue; // editor / debate windows: no status heuristics, no nudges
            }
            let status = detect_status(&a.screen_text()).to_string();
            if status != a.status {
                a.status = status.clone();
                let _ = self.db.set_status(&name, &status);
            }
            if status != "idle" {
                self.idle_since.remove(&name);
                continue;
            }
            let since = *self.idle_since.entry(name.clone()).or_insert(now);
            if now.duration_since(since) < IDLE_GRACE {
                continue;
            }
            let pending = self.db.pending_nudges(&name).unwrap_or_default();
            if !pending.is_empty() {
                let text = format_nudge(&pending);
                a.type_line(&text);
                let _ = self.db.mark_nudged(&pending.iter().map(|m| m.id).collect::<Vec<_>>());
                self.idle_since.insert(name.clone(), now);
                info!(agent = %name, n = pending.len(), "nudged");
            }
        }
        self.publish_agents(false);
    }

    /// Send frames to clients whose shown agent changed.
    fn flush_frames(&mut self) {
        let mut frames: HashMap<String, crate::proto::Frame> = HashMap::new();
        let mut want: Vec<(u64, String)> = Vec::new();
        for (id, c) in self.clients.iter() {
            if let Some(name) = &c.subscribed {
                if let Some(a) = self.agents.get(name) {
                    if a.dirty || c.needs_frame {
                        want.push((*id, name.clone()));
                    }
                }
            }
        }
        for (id, name) in want {
            let frame = frames.entry(name.clone()).or_insert_with(|| self.agents[&name].frame()).clone();
            if let Some(c) = self.clients.get_mut(&id) {
                c.needs_frame = false;
                let _ = c.tx.send(DaemonMsg::Frame(frame));
            }
        }
        for a in self.agents.values_mut() {
            a.dirty = false;
        }
    }
}

fn flatten(s: &str) -> String {
    let mut out = String::new();
    let mut last_space = false;
    for ch in s.chars() {
        if ch == '\n' || ch == '\r' {
            if !out.ends_with(" ⏎ ") {
                out.push_str(" ⏎ ");
            }
            last_space = true;
        } else if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out.trim().to_string()
}

pub fn format_nudge(msgs: &[db::Message]) -> String {
    let mut text = if msgs.len() == 1 {
        format!("[qoral] message from {}: {}", msgs[0].sender, flatten(&msgs[0].body))
    } else {
        let parts: Vec<String> = msgs.iter().enumerate().map(|(i, m)| format!("({}) from {}: {}", i + 1, m.sender, flatten(&m.body))).collect();
        format!("[qoral] {} new messages: {}", msgs.len(), parts.join("  "))
    };
    let mut truncated = false;
    if text.chars().count() > MAX_NUDGE_CHARS {
        text = text.chars().take(MAX_NUDGE_CHARS - 1).collect::<String>() + "…";
        truncated = true;
    }
    let mut senders: Vec<&str> = msgs.iter().map(|m| m.sender.as_str()).collect();
    senders.dedup();
    let hint = if senders.len() == 1 { format!("Reply with qoral_send(to=\"{}\").", senders[0]) } else { "Reply to each sender with qoral_send.".to_string() };
    if truncated {
        text.push_str(" [truncated; call qoral_inbox for the full text.]");
    }
    format!("{text} {hint}")
}

async fn handle_client(stream: UnixStream, state: Shared) {
    let (mut rd, mut wr) = stream.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel::<DaemonMsg>();
    let id = state.lock().unwrap().add_client(tx.clone());
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if crate::proto::write_msg(&mut wr, &m).await.is_err() {
                break;
            }
        }
    });
    let _ = tx.send(DaemonMsg::Hello { version: env!("CARGO_PKG_VERSION").into(), pid: std::process::id() });
    {
        let s = state.lock().unwrap();
        let _ = tx.send(DaemonMsg::Agents(s.agent_infos()));
    }
    loop {
        let msg: ClientMsg = match crate::proto::read_msg(&mut rd).await {
            Ok(m) => m,
            Err(_) => break,
        };
        handle_msg(msg, id, &state, &tx);
    }
    {
        let mut s = state.lock().unwrap();
        s.clients.remove(&id);
    }
    writer.abort();
}

/// Synchronous: takes the lock, applies one client message, releases. Never awaits.
fn handle_msg(msg: ClientMsg, id: u64, state: &Shared, tx: &mpsc::UnboundedSender<DaemonMsg>) {
    let mut s = state.lock().unwrap();
    {
        match msg {
            ClientMsg::Hello { .. } => {}
            ClientMsg::Ping => {
                let _ = tx.send(DaemonMsg::Pong);
            }
            ClientMsg::ListAgents => {
                let _ = tx.send(DaemonMsg::Agents(s.agent_infos()));
            }
            ClientMsg::Subscribe { agent } => {
                let (cols, rows) = { let c = &s.clients[&id]; (c.cols, c.rows) };
                if let Some(name) = &agent {
                    if let Some(a) = s.agents.get_mut(name) {
                        a.resize(cols, rows);
                        a.dirty = true;
                    }
                }
                if let Some(c) = s.clients.get_mut(&id) {
                    c.subscribed = agent;
                    c.needs_frame = true;
                }
            }
            ClientMsg::Resize { cols, rows } => {
                let sub = if let Some(c) = s.clients.get_mut(&id) {
                    c.cols = cols;
                    c.rows = rows;
                    c.needs_frame = true;
                    c.subscribed.clone()
                } else {
                    None
                };
                if let Some(name) = sub {
                    if let Some(a) = s.agents.get_mut(&name) {
                        a.resize(cols, rows);
                    }
                }
            }
            ClientMsg::Input { agent, data } => {
                if let Some(a) = s.agents.get_mut(&agent) {
                    a.write(data);
                }
            }
            ClientMsg::Scroll { agent, delta } => {
                if let Some(a) = s.agents.get_mut(&agent) {
                    a.scroll(delta);
                }
            }
            ClientMsg::Spawn(req) => {
                let size = s.clients.get(&id).map(|c| (c.cols, c.rows));
                match s.spawn_agent(&req, size) {
                    Ok((name, notices)) => {
                        let _ = tx.send(DaemonMsg::Spawned { name: name.clone(), notices });
                        if req.focus {
                            s.broadcast(DaemonMsg::Focused { name });
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(DaemonMsg::Error { message: e.to_string() });
                    }
                }
            }
            ClientMsg::Kill { name } => match s.kill_agent(&name, true) {
                Ok(_) => {
                    let _ = tx.send(DaemonMsg::Ok);
                }
                Err(e) => {
                    let _ = tx.send(DaemonMsg::Error { message: e.to_string() });
                }
            },
            ClientMsg::Document { name } => match s.document_agent(&name) {
                Ok(()) => {
                    let _ = tx.send(DaemonMsg::Ok);
                }
                Err(e) => {
                    let _ = tx.send(DaemonMsg::Error { message: e.to_string() });
                }
            },
            ClientMsg::Focus { name } => {
                if s.agents.contains_key(&name) {
                    s.broadcast(DaemonMsg::Focused { name });
                    let _ = tx.send(DaemonMsg::Ok);
                } else {
                    let _ = tx.send(DaemonMsg::Error { message: format!("no agent named \"{name}\"") });
                }
            }
            ClientMsg::Shutdown => {
                info!("shutdown requested");
                let names: Vec<String> = s.agents.keys().cloned().collect();
                for n in names {
                    if let Some(mut a) = s.agents.remove(&n) {
                        a.kill();
                        let _ = s.db.mark_exited(&n);
                    }
                }
                s.broadcast(DaemonMsg::Shutdown);
                s.shutting_down = true;
                let _ = tx.send(DaemonMsg::Ok);
            }
        }
    }
}

pub async fn run() -> Result<()> {
    paths::ensure_dirs()?;
    let sock = paths::socket_path();
    if sock.exists() {
        // stale socket from a dead daemon? try connecting first
        if UnixStream::connect(&sock).await.is_ok() {
            bail!("a qoral daemon is already running on {}", sock.display());
        }
        let _ = std::fs::remove_file(&sock);
    }
    let listener = UnixListener::bind(&sock).with_context(|| format!("bind {}", sock.display()))?;
    info!(socket = %sock.display(), pid = std::process::id(), "daemon up");

    let (bytes_tx, mut bytes_rx) = mpsc::unbounded_channel::<(String, Vec<u8>)>();
    let db = Db::open()?;
    // rows from a previous daemon life are no longer backed by processes
    for a in db.list_agents(true).unwrap_or_default() {
        if a.harness != "moderator" {
            let _ = db.mark_exited(&a.name);
        }
    }
    let state: Shared = Arc::new(Mutex::new(State {
        agents: HashMap::new(),
        clients: HashMap::new(),
        db,
        next_client: 1,
        idle_since: HashMap::new(),
        last_agents: Vec::new(),
        bytes_tx,
        shutting_down: false,
    }));

    // PTY output -> emulators
    {
        let state = state.clone();
        tokio::spawn(async move {
            while let Some((name, bytes)) = bytes_rx.recv().await {
                let mut s = state.lock().unwrap();
                if let Some(a) = s.agents.get_mut(&name) {
                    a.advance(&bytes);
                }
            }
        });
    }
    // frames at ~60 fps
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(16));
            loop {
                iv.tick().await;
                state.lock().unwrap().flush_frames();
            }
        });
    }
    // housekeeping
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(400));
            loop {
                iv.tick().await;
                let shutting_down = {
                    let mut s = state.lock().unwrap();
                    s.tick(&state);
                    s.shutting_down
                };
                if shutting_down {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    info!("bye");
                    let _ = std::fs::remove_file(paths::socket_path());
                    std::process::exit(0);
                }
            }
        });
    }
    // signals
    {
        let state = state.clone();
        let sock = sock.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            warn!("SIGINT: killing agents");
            let mut s = state.lock().unwrap();
            let names: Vec<String> = s.agents.keys().cloned().collect();
            for n in names {
                if let Some(mut a) = s.agents.remove(&n) {
                    a.kill();
                    let _ = s.db.mark_exited(&n);
                }
            }
            s.broadcast(DaemonMsg::Shutdown);
            let _ = std::fs::remove_file(&sock);
            std::process::exit(0);
        });
    }

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(handle_client(stream, state.clone()));
    }
}
