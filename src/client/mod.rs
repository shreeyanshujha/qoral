//! The interactive client: sidebar + live agent viewport, attached to the daemon.
pub mod keys;
pub mod ui;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use crossterm::{execute, terminal};
use futures::StreamExt;
use ratatui::layout::Rect;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::ctl;
use crate::db::{self, Db, Message};
use crate::harness::{self_exe, shq};
use crate::proto::{read_msg, write_msg, AgentInfo, ClientMsg, DaemonMsg, Frame, SpawnReq};
use crate::theme::{self, Theme};

pub const SIDEBAR_W: u16 = 38;

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Focus {
    Sidebar,
    Main,
}

#[derive(Clone)]
pub enum Pending {
    SpawnHarness,
    SpawnName { harness: String },
    SpawnDir { harness: String, name: String },
    SpawnPrompt { harness: String, name: String, dir: String },
    Message { to: String },
    Broadcast,
    Kill { name: String },
    QuitAll,
    DebateQuestion,
    DebateDir { question: String },
    DebateMode { question: String, dir: String },
    DebateBuild { question: String, dir: String },
}

pub enum Mode {
    List,
    Input { label: String, value: String, placeholder: String, pending: Pending },
    Pick { label: String, options: Vec<(char, String)>, pending: Pending },
    Confirm { text: String, pending: Pending },
    Log,
    Help,
}

pub struct App {
    pub agents: Vec<AgentInfo>,
    pub frame: Option<Frame>,
    pub shown: Option<String>,
    pub focus: Focus,
    pub sel: usize,
    pub mode: Mode,
    pub flash: Option<(String, Instant)>,
    pub bus: Vec<Message>,
    pub bus_full: Vec<Message>,
    pub human_unread: u32,
    pub spin: usize,
    pub main_area: Rect,
    pub sidebar_area: Rect,
    pub log_scroll: usize,
    pub last_dir: PathBuf,
    pub quit: bool,
    pub theme: Theme,
    tx: mpsc::UnboundedSender<ClientMsg>,
    sent_size: (u16, u16),
    db: Db,
}

impl App {
    fn send(&self, m: ClientMsg) {
        let _ = self.tx.send(m);
    }
    fn flash(&mut self, s: impl Into<String>, ms: u64) {
        self.flash = Some((s.into(), Instant::now() + Duration::from_millis(ms)));
    }
    fn selected(&self) -> Option<&AgentInfo> {
        self.agents.get(self.sel)
    }
    fn ensure_size(&mut self) {
        let size = (self.main_area.width, self.main_area.height);
        if size != self.sent_size && size.0 > 0 && size.1 > 0 {
            self.sent_size = size;
            self.send(ClientMsg::Resize { cols: size.0, rows: size.1 });
        }
    }
    fn show(&mut self, name: &str) {
        self.ensure_size();
        self.shown = Some(name.to_string());
        self.frame = None;
        self.send(ClientMsg::Subscribe { agent: Some(name.to_string()) });
        if let Some(i) = self.agents.iter().position(|a| a.name == name) {
            self.sel = i;
        }
        self.focus = Focus::Main;
    }
    fn cycle(&mut self, dir: i32) {
        let live: Vec<usize> = self.agents.iter().enumerate().filter(|(_, a)| a.exited_at.is_none() && a.harness != "moderator").map(|(i, _)| i).collect();
        if live.is_empty() {
            return;
        }
        let cur = self.shown.as_ref().and_then(|s| self.agents.iter().position(|a| &a.name == s));
        let pos = cur.and_then(|c| live.iter().position(|&i| i == c));
        let next = match pos {
            Some(p) => live[((p as i32 + dir).rem_euclid(live.len() as i32)) as usize],
            None => live[0],
        };
        let name = self.agents[next].name.clone();
        self.show(&name);
    }
    fn refresh_bus(&mut self) {
        self.bus = self.db.recent_messages(12).unwrap_or_default();
        self.human_unread = self.db.count_unread("human").unwrap_or(0);
        if matches!(self.mode, Mode::Log) {
            self.bus_full = self.db.recent_messages(300).unwrap_or_default();
            let ids: Vec<i64> = self.bus_full.iter().filter(|m| m.recipient == "human" && m.read_at.is_none()).map(|m| m.id).collect();
            let _ = self.db.mark_read(&ids);
        }
    }

    // ---- wizards ----
    fn start_spawn(&mut self) {
        self.mode = Mode::Pick {
            label: "harness?".into(),
            options: vec![('c', "claude".into()), ('x', "codex".into()), ('a', "agy".into()), ('g', "gemini".into())],
            pending: Pending::SpawnHarness,
        };
    }
    fn start_message(&mut self, broadcast: bool) {
        if broadcast {
            self.mode = Mode::Input { label: "to all".into(), value: String::new(), placeholder: "type a message".into(), pending: Pending::Broadcast };
            return;
        }
        match self.selected() {
            Some(a) if a.exited_at.is_none() && a.harness != "moderator" => {
                let to = a.name.clone();
                self.mode = Mode::Input { label: format!("to {to}"), value: String::new(), placeholder: "type a message".into(), pending: Pending::Message { to } };
            }
            _ => self.flash("select a running agent first", 3000),
        }
    }
    fn start_kill(&mut self) {
        let Some(a) = self.selected().cloned() else { return };
        if a.exited_at.is_some() || a.harness == "cmd" {
            self.send(ClientMsg::Kill { name: a.name });
            return;
        }
        self.mode = Mode::Confirm { text: format!("kill {}? (y/N)", a.name), pending: Pending::Kill { name: a.name } };
    }

    fn project_dir(&self) -> PathBuf {
        match self.selected() {
            Some(a) if a.harness != "moderator" => PathBuf::from(&a.cwd),
            _ => self.last_dir.clone(),
        }
    }

    /// Open a plain command in its own window (editor, debate moderator …).
    fn open_window(&mut self, name: &str, cwd: &std::path::Path, script: String) {
        self.ensure_size();
        let wrapped = format!("{script}; echo; echo '[qoral] finished. Press Enter to close this window.'; read _");
        self.send(ClientMsg::Spawn(SpawnReq {
            harness: "cmd".into(),
            name: Some(name.into()),
            cwd: Some(cwd.display().to_string()),
            prompt: None,
            focus: true,
            command: Some(vec!["sh".into(), "-c".into(), wrapped]),
        }));
    }

    fn open_knowledge(&mut self) {
        let cwd = self.project_dir();
        let file = cwd.join(".qoral/KNOWLEDGE.md");
        let script = format!(
            "if [ -f {f} ]; then ${{EDITOR:-less}} {f}; else echo 'no knowledge yet at {f}'; echo; echo '(sessions are documented when they end; agents can also call qoral_remember)'; fi",
            f = shq(&file.display().to_string())
        );
        let name = format!("knowledge-{:x}", db::now_ms() % 4096);
        self.open_window(&name, &cwd, script);
    }

    fn start_debate(&mut self) {
        self.mode = Mode::Input { label: "debate".into(), value: String::new(), placeholder: "question to debate".into(), pending: Pending::DebateQuestion };
    }

    fn submit_input(&mut self, pending: Pending, value: String) {
        let v = value.trim().to_string();
        match pending {
            Pending::SpawnName { harness } => {
                let taken: Vec<String> = self.agents.iter().map(|a| a.name.clone()).collect();
                let name = if v.is_empty() { crate::harness::suggest_name(&taken) } else { v };
                let def = crate::paths::short_dir(&self.last_dir);
                self.mode = Mode::Input { label: "dir".into(), value: String::new(), placeholder: def, pending: Pending::SpawnDir { harness, name } };
            }
            Pending::SpawnDir { harness, name } => {
                let dir = if v.is_empty() { self.last_dir.display().to_string() } else { v };
                self.mode = Mode::Input { label: "task".into(), value: String::new(), placeholder: "(optional)".into(), pending: Pending::SpawnPrompt { harness, name, dir } };
            }
            Pending::SpawnPrompt { harness, name, dir } => {
                self.mode = Mode::List;
                self.last_dir = crate::paths::expand_home(&dir);
                self.ensure_size();
                self.send(ClientMsg::Spawn(SpawnReq { harness, name: Some(name), cwd: Some(dir), prompt: if v.is_empty() { None } else { Some(v) }, focus: true, command: None }));
            }
            Pending::DebateQuestion => {
                if v.is_empty() {
                    self.mode = Mode::List;
                    return;
                }
                let def = crate::paths::short_dir(&self.project_dir());
                self.mode = Mode::Input { label: "dir".into(), value: String::new(), placeholder: def, pending: Pending::DebateDir { question: v } };
            }
            Pending::DebateDir { question } => {
                let dir = if v.is_empty() { self.project_dir().display().to_string() } else { v };
                self.mode = Mode::Pick {
                    label: "outcome?".into(),
                    options: vec![('d', "decide (converge on one)".into()), ('o', "options (menu, you choose)".into())],
                    pending: Pending::DebateMode { question, dir },
                };
            }
            Pending::Message { to } => {
                self.mode = Mode::List;
                if v.is_empty() {
                    return;
                }
                match db::send_message(&self.db, "human", &to, &v) {
                    Ok(t) => self.flash(format!("sent to {}", t.join(", ")), 3000),
                    Err(e) => self.flash(format!("error: {e}"), 6000),
                }
            }
            Pending::Broadcast => {
                self.mode = Mode::List;
                if v.is_empty() {
                    return;
                }
                match db::send_message(&self.db, "human", "all", &v) {
                    Ok(t) if t.is_empty() => self.flash("no agents to send to", 3000),
                    Ok(t) => self.flash(format!("sent to {}", t.join(", ")), 3000),
                    Err(e) => self.flash(format!("error: {e}"), 6000),
                }
            }
            _ => self.mode = Mode::List,
        }
    }

    fn handle_daemon(&mut self, m: DaemonMsg) {
        match m {
            DaemonMsg::Agents(v) => {
                self.agents = v;
                if self.sel >= self.agents.len() {
                    self.sel = self.agents.len().saturating_sub(1);
                }
                if let Some(s) = &self.shown {
                    if !self.agents.iter().any(|a| &a.name == s) {
                        self.shown = None;
                        self.frame = None;
                        self.send(ClientMsg::Subscribe { agent: None });
                        self.focus = Focus::Sidebar;
                    }
                }
            }
            DaemonMsg::Frame(f) => {
                if Some(&f.agent) == self.shown.as_ref() {
                    self.frame = Some(f);
                }
            }
            DaemonMsg::Spawned { name, notices } => {
                if notices.is_empty() {
                    self.flash(format!("started {name}"), 3000);
                } else {
                    self.flash(format!("started {name} · {}", notices[0]), 9000);
                }
            }
            DaemonMsg::Focused { name } => self.show(&name),
            DaemonMsg::Error { message } => self.flash(format!("error: {message}"), 6000),
            DaemonMsg::Shutdown => self.quit = true,
            _ => {}
        }
    }

    fn handle_key(&mut self, ev: KeyEvent) {
        if ev.kind != KeyEventKind::Press && ev.kind != KeyEventKind::Repeat {
            return;
        }
        let alt = ev.modifiers.contains(KeyModifiers::ALT);
        // global chords
        if alt {
            match ev.code {
                KeyCode::Left | KeyCode::Char('h') => {
                    self.focus = Focus::Sidebar;
                    return;
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    if self.shown.is_some() {
                        self.focus = Focus::Main;
                    } else if let Some(a) = self.selected().cloned() {
                        self.show(&a.name);
                    }
                    return;
                }
                KeyCode::Char(']') => return self.cycle(1),
                KeyCode::Char('[') => return self.cycle(-1),
                KeyCode::Char(c @ '1'..='9') => {
                    let i = c as usize - '1' as usize;
                    if let Some(a) = self.agents.get(i).cloned() {
                        self.show(&a.name);
                    }
                    return;
                }
                KeyCode::Char('n') => {
                    self.focus = Focus::Sidebar;
                    self.start_spawn();
                    return;
                }
                KeyCode::Char('m') => {
                    self.focus = Focus::Sidebar;
                    self.start_message(false);
                    return;
                }
                KeyCode::Char('d') => {
                    self.quit = true;
                    return;
                }
                _ => {}
            }
        }
        if self.focus == Focus::Main {
            if let Some(name) = self.shown.clone() {
                let mode = self.frame.as_ref().map(|f| f.mode).unwrap_or(0);
                if let Some(bytes) = keys::encode_key(&ev, mode) {
                    self.send(ClientMsg::Input { agent: name, data: bytes });
                }
            } else {
                self.focus = Focus::Sidebar;
            }
            return;
        }
        self.handle_sidebar_key(ev);
    }

    fn handle_sidebar_key(&mut self, ev: KeyEvent) {
        let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
        // take the mode out so we can mutate self freely
        let mode = std::mem::replace(&mut self.mode, Mode::List);
        match mode {
            Mode::Input { label, mut value, placeholder, pending } => {
                match ev.code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => self.submit_input(pending, value),
                    KeyCode::Backspace => {
                        value.pop();
                        self.mode = Mode::Input { label, value, placeholder, pending };
                    }
                    KeyCode::Char('u') if ctrl => {
                        value.clear();
                        self.mode = Mode::Input { label, value, placeholder, pending };
                    }
                    KeyCode::Char('w') if ctrl => {
                        let trimmed = value.trim_end().to_string();
                        let cut = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
                        value = trimmed[..cut].to_string();
                        self.mode = Mode::Input { label, value, placeholder, pending };
                    }
                    KeyCode::Char('c') if ctrl => {}
                    KeyCode::Char(c) => {
                        value.push(c);
                        self.mode = Mode::Input { label, value, placeholder, pending };
                    }
                    _ => self.mode = Mode::Input { label, value, placeholder, pending },
                }
            }
            Mode::Pick { label, options, pending } => match ev.code {
                KeyCode::Esc | KeyCode::Char('q') => {}
                KeyCode::Char(c) if options.iter().any(|(k, _)| *k == c) => {
                    let picked = options.iter().find(|(k, _)| *k == c).unwrap().1.clone();
                    match pending {
                        Pending::SpawnHarness => {
                            let taken: Vec<String> = self.agents.iter().map(|a| a.name.clone()).collect();
                            let def = crate::harness::suggest_name(&taken);
                            self.mode = Mode::Input { label: "name".into(), value: String::new(), placeholder: def, pending: Pending::SpawnName { harness: picked } };
                        }
                        Pending::DebateMode { question, dir } => {
                            if c == 'o' {
                                let cwd = crate::paths::expand_home(&dir);
                                self.last_dir = cwd.clone();
                                let script = format!("{} debate {} --dir {} --options", shq(&self_exe()), shq(&question), shq(&cwd.display().to_string()));
                                let name = format!("options-{:x}", db::now_ms() % 4096);
                                self.open_window(&name, &cwd, script);
                                self.flash("options exploration started · moderator output on the right", 5000);
                            } else {
                                self.mode = Mode::Pick {
                                    label: "build the decision afterwards?".into(),
                                    options: vec![('y', "yes, spawn a builder".into()), ('n', "no, decide only".into())],
                                    pending: Pending::DebateBuild { question, dir },
                                };
                            }
                        }
                        Pending::DebateBuild { question, dir } => {
                            let build = c == 'y';
                            let cwd = crate::paths::expand_home(&dir);
                            self.last_dir = cwd.clone();
                            let script = format!("{} debate {} --dir {}{}", shq(&self_exe()), shq(&question), shq(&cwd.display().to_string()), if build { " --build" } else { "" });
                            let name = format!("debate-{:x}", db::now_ms() % 4096);
                            self.open_window(&name, &cwd, script);
                            self.flash("debate started · moderator output on the right", 5000);
                        }
                        _ => {}
                    }
                }
                _ => self.mode = Mode::Pick { label, options, pending },
            },
            Mode::Confirm { text, pending } => {
                let yes = matches!(ev.code, KeyCode::Char('y') | KeyCode::Char('Y'));
                if yes {
                    match pending {
                        Pending::Kill { name } => {
                            self.send(ClientMsg::Kill { name: name.clone() });
                            self.flash(format!("killed {name}"), 3000);
                        }
                        Pending::QuitAll => self.send(ClientMsg::Shutdown),
                        _ => {}
                    }
                }
                let _ = text;
            }
            Mode::Log => {
                self.mode = Mode::Log;
                match ev.code {
                    KeyCode::Char('j') | KeyCode::Down => self.log_scroll = self.log_scroll.saturating_sub(1),
                    KeyCode::Char('k') | KeyCode::Up => self.log_scroll += 1,
                    KeyCode::Char('G') => self.log_scroll = 0,
                    KeyCode::Char('g') => self.log_scroll = usize::MAX / 2,
                    KeyCode::PageDown => self.log_scroll = self.log_scroll.saturating_sub(10),
                    KeyCode::PageUp => self.log_scroll += 10,
                    KeyCode::Char('l') | KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::List,
                    _ => {}
                }
            }
            Mode::Help => {
                self.mode = Mode::Help;
                if matches!(ev.code, KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q')) {
                    self.mode = Mode::List;
                }
            }
            Mode::List => {
                self.mode = Mode::List;
                match ev.code {
                    KeyCode::Char('j') | KeyCode::Down => self.sel = (self.sel + 1).min(self.agents.len().saturating_sub(1)),
                    KeyCode::Char('k') | KeyCode::Up => self.sel = self.sel.saturating_sub(1),
                    KeyCode::Enter | KeyCode::Right => {
                        if let Some(a) = self.selected().cloned() {
                            if a.harness != "moderator" {
                                self.show(&a.name);
                            }
                        }
                    }
                    KeyCode::Char('n') => self.start_spawn(),
                    KeyCode::Char('m') => self.start_message(false),
                    KeyCode::Char('b') => self.start_message(true),
                    KeyCode::Char('x') => self.start_kill(),
                    KeyCode::Char('l') => {
                        self.mode = Mode::Log;
                        self.log_scroll = 0;
                        self.refresh_bus();
                    }
                    KeyCode::Char('?') => self.mode = Mode::Help,
                    KeyCode::Char('d') => self.quit = true,
                    KeyCode::Char('Q') => self.mode = Mode::Confirm { text: "quit qoral and kill every agent? (y/N)".into(), pending: Pending::QuitAll },
                    KeyCode::Char('D') => self.start_debate(),
                    KeyCode::Char('o') => self.open_knowledge(),
                    KeyCode::Char('c') if ctrl => self.flash("d detaches · Q quits all", 3000),
                    KeyCode::Char(c @ '1'..='9') => {
                        let i = c as usize - '1' as usize;
                        if let Some(a) = self.agents.get(i).cloned() {
                            self.show(&a.name);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_mouse(&mut self, ev: crossterm::event::MouseEvent) {
        let in_main = ev.column >= self.main_area.x && ev.row >= self.main_area.y && ev.row < self.main_area.bottom();
        let in_sidebar = ev.column < self.sidebar_area.right();
        match ev.kind {
            MouseEventKind::Down(_) if in_sidebar => {
                self.focus = Focus::Sidebar;
                let row = ev.row.saturating_sub(self.sidebar_area.y + 2) as usize;
                if row < self.agents.len() {
                    self.sel = row;
                }
            }
            _ if in_main => {
                if matches!(ev.kind, MouseEventKind::Down(_)) {
                    self.focus = Focus::Main;
                }
                let Some(name) = self.shown.clone() else { return };
                let mode = self.frame.as_ref().map(|f| f.mode).unwrap_or(0);
                if let Some(bytes) = keys::encode_mouse(&ev, (self.main_area.x, self.main_area.y), mode) {
                    self.send(ClientMsg::Input { agent: name, data: bytes });
                } else {
                    match ev.kind {
                        MouseEventKind::ScrollUp => self.send(ClientMsg::Scroll { agent: name, delta: 3 }),
                        MouseEventKind::ScrollDown => self.send(ClientMsg::Scroll { agent: name, delta: -3 }),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

pub async fn run(initial: Option<String>) -> Result<()> {
    let stream = ctl::connect().await?;
    let (mut rd, mut wr) = stream.into_split();
    let (tx, mut out_rx) = mpsc::unbounded_channel::<ClientMsg>();
    let (in_tx, mut in_rx) = mpsc::unbounded_channel::<DaemonMsg>();
    tokio::spawn(async move {
        while let Some(m) = out_rx.recv().await {
            if write_msg(&mut wr, &m).await.is_err() {
                break;
            }
        }
    });
    tokio::spawn(async move {
        loop {
            match read_msg::<_, DaemonMsg>(&mut rd).await {
                Ok(m) => {
                    if in_tx.send(m).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = in_tx.send(DaemonMsg::Shutdown);
                    break;
                }
            }
        }
    });
    let _ = tx.send(ClientMsg::Hello { client: "tui".into() });

    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), crossterm::event::EnableMouseCapture, crossterm::event::EnableBracketedPaste)?;

    let mut app = App {
        agents: Vec::new(),
        frame: None,
        shown: None,
        focus: Focus::Sidebar,
        sel: 0,
        mode: Mode::List,
        flash: None,
        bus: Vec::new(),
        bus_full: Vec::new(),
        human_unread: 0,
        spin: 0,
        main_area: Rect::default(),
        sidebar_area: Rect::default(),
        log_scroll: 0,
        last_dir: std::env::current_dir().unwrap_or_else(|_| dirs::home_dir().unwrap_or_default()),
        quit: false,
        theme: theme::load(),
        tx: tx.clone(),
        sent_size: (0, 0),
        db: Db::open()?,
    };
    app.refresh_bus();
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut pending_initial = initial;

    let result: Result<()> = async {
        loop {
            terminal.draw(|f| ui::draw(f, &mut app))?;
            app.ensure_size();
            if let Some(name) = pending_initial.take() {
                app.show(&name);
            }
            if app.quit {
                break;
            }
            tokio::select! {
                ev = events.next() => {
                    match ev {
                        Some(Ok(Event::Key(k))) => app.handle_key(k),
                        Some(Ok(Event::Mouse(m))) => app.handle_mouse(m),
                        Some(Ok(Event::Paste(s))) => {
                            match (&app.mode, app.focus) {
                                (Mode::Input { .. }, _) => {
                                    if let Mode::Input { value, .. } = &mut app.mode { value.push_str(&s.replace(['\r', '\n'], " ")); }
                                }
                                (_, Focus::Main) => {
                                    if let Some(name) = app.shown.clone() {
                                        let mode = app.frame.as_ref().map(|f| f.mode).unwrap_or(0);
                                        app.send(ClientMsg::Input { agent: name, data: keys::encode_paste(&s, mode) });
                                    }
                                }
                                _ => {}
                            }
                        }
                        Some(Ok(Event::Resize(_, _))) => {}
                        Some(Ok(_)) => {}
                        Some(Err(e)) => return Err(e.into()),
                        None => break,
                    }
                    // drain any immediately-available events before redrawing
                }
                Some(m) = in_rx.recv() => {
                    app.handle_daemon(m);
                    // coalesce bursts of frames
                    while let Ok(m) = in_rx.try_recv() { app.handle_daemon(m); }
                }
                _ = tick.tick() => {
                    app.spin += 1;
                    app.refresh_bus();
                    if let Some((_, until)) = &app.flash { if Instant::now() > *until { app.flash = None; } }
                }
            }
        }
        Ok(())
    }
    .await;

    let _ = execute!(std::io::stdout(), crossterm::event::DisableMouseCapture, crossterm::event::DisableBracketedPaste);
    ratatui::restore();
    let _ = terminal::disable_raw_mode();
    result
}
