//! One agent: a child process in a PTY plus a terminal emulator holding its screen and scrollback.
use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::thread;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{self, Config as TermConfig, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as AColor, NamedColor, Processor};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc;

use crate::db;
use crate::harness::{self, Launch};
use crate::proto::{self, attr, mode, Cell, Color, Frame};

#[derive(Clone)]
pub struct Proxy;
impl EventListener for Proxy {
    fn send_event(&self, _event: Event) {}
}

/// Simple `Dimensions` impl for sizing the emulator.
#[derive(Clone, Copy)]
pub struct Size {
    pub cols: usize,
    pub rows: usize,
}
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

pub struct Agent {
    pub name: String,
    pub harness: String,
    pub cwd: PathBuf,
    pub task: Option<String>,
    pub created_at: i64,
    pub exited_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub status: String,
    pub dirty: bool,
    pub cols: u16,
    pub rows: u16,
    pub launch: Launch,
    term: Term<Proxy>,
    parser: Processor,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    input_tx: std::sync::mpsc::Sender<Vec<u8>>,
}

impl Agent {
    /// Spawn the harness in a PTY. Bytes it prints are forwarded to `bytes_tx` as (name, bytes).
    pub fn spawn(
        name: &str,
        harness_name: &str,
        cwd: &Path,
        prompt: Option<&str>,
        cols: u16,
        rows: u16,
        bytes_tx: mpsc::UnboundedSender<(String, Vec<u8>)>,
    ) -> Result<Agent> {
        let launch = harness::build_launch(harness_name, name, cwd, prompt)?;
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .context("openpty")?;

        let mut cmd = CommandBuilder::new(&launch.argv[0]);
        cmd.args(&launch.argv[1..]);
        cmd.cwd(cwd);
        cmd.env("QORAL_AGENT", name);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (k, v) in &launch.env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).with_context(|| format!("spawn {}", launch.argv[0]))?;
        drop(pair.slave);
        let killer = child.clone_killer();

        // reader thread: PTY -> daemon
        let mut reader = pair.master.try_clone_reader().context("clone reader")?;
        let rname = name.to_string();
        thread::Builder::new().name(format!("pty-read-{name}")).spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if bytes_tx.send((rname.clone(), buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                }
            }
        })?;

        // writer thread: daemon -> PTY (keeps the blocking write off the async runtime)
        let mut writer = pair.master.take_writer().context("take writer")?;
        let (input_tx, input_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        thread::Builder::new().name(format!("pty-write-{name}")).spawn(move || {
            while let Ok(data) = input_rx.recv() {
                if writer.write_all(&data).is_err() || writer.flush().is_err() {
                    break;
                }
            }
        })?;

        let config = TermConfig { scrolling_history: 20_000, ..TermConfig::default() };
        let size = Size { cols: cols as usize, rows: rows as usize };
        let term = Term::new(config, &size, Proxy);

        Ok(Agent {
            name: name.to_string(),
            harness: harness_name.to_string(),
            cwd: cwd.to_path_buf(),
            task: prompt.map(|p| p.chars().take(200).collect()),
            created_at: db::now_ms(),
            exited_at: None,
            exit_code: None,
            status: "starting".into(),
            dirty: true,
            cols,
            rows,
            launch,
            term,
            parser: Processor::new(),
            master: pair.master,
            killer,
            child,
            input_tx,
        })
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
        self.dirty = true;
    }

    /// Feed text into the emulator only (not the child): banners like "agent exited".
    pub fn banner(&mut self, text: &str) {
        let s = format!("\r\n\x1b[33m{text}\x1b[0m\r\n");
        self.advance(s.as_bytes());
    }

    pub fn write(&mut self, data: Vec<u8>) {
        if self.exited_at.is_some() {
            return;
        }
        self.term.scroll_display(Scroll::Bottom);
        let _ = self.input_tx.send(data);
    }

    /// Type a line the way a human would: text, short pause, Enter.
    pub fn type_line(&mut self, text: &str) {
        self.write(text.as_bytes().to_vec());
        let tx = self.input_tx.clone();
        thread::spawn(move || {
            thread::sleep(std::time::Duration::from_millis(350));
            let _ = tx.send(b"\r".to_vec());
        });
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 || (cols == self.cols && rows == self.rows) {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.term.resize(Size { cols: cols as usize, rows: rows as usize });
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        self.dirty = true;
    }

    pub fn scroll(&mut self, delta: i32) {
        self.term.scroll_display(Scroll::Delta(delta));
        self.dirty = true;
    }

    /// The visible screen (ignoring any scrollback offset), as plain text lines.
    pub fn screen_text(&self) -> String {
        let grid = self.term.grid();
        let mut out = String::new();
        for l in 0..self.rows as i32 {
            let row = &grid[Line(l)];
            let mut line = String::new();
            for c in 0..self.cols as usize {
                let cell = &row[Column(c)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                line.push(cell.c);
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }

    pub fn poll_exit(&mut self) -> Option<i32> {
        if self.exited_at.is_some() {
            return self.exit_code;
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                let code = status.exit_code() as i32;
                self.exited_at = Some(db::now_ms());
                self.exit_code = Some(code);
                self.status = "exited".into();
                Some(code)
            }
            _ => None,
        }
    }

    pub fn kill(&mut self) {
        let _ = self.killer.kill();
        let _ = self.child.wait();
        if self.exited_at.is_none() {
            self.exited_at = Some(db::now_ms());
            self.status = "exited".into();
        }
    }

    fn mode_bits(m: TermMode) -> u32 {
        let mut bits = 0;
        if m.contains(TermMode::APP_CURSOR) {
            bits |= mode::APP_CURSOR;
        }
        if m.contains(TermMode::APP_KEYPAD) {
            bits |= mode::APP_KEYPAD;
        }
        if m.contains(TermMode::BRACKETED_PASTE) {
            bits |= mode::BRACKETED_PASTE;
        }
        if m.intersects(TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) {
            bits |= mode::MOUSE;
        }
        if m.contains(TermMode::MOUSE_MOTION) {
            bits |= mode::MOUSE_MOTION;
        }
        if m.contains(TermMode::SGR_MOUSE) {
            bits |= mode::SGR_MOUSE;
        }
        if m.contains(TermMode::ALT_SCREEN) {
            bits |= mode::ALT_SCREEN;
        }
        bits
    }

    fn color(c: AColor, attrs: &mut u8) -> Color {
        match c {
            AColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
            AColor::Indexed(i) => Color::Indexed(i),
            AColor::Named(n) => match n {
                NamedColor::Black => Color::Indexed(0),
                NamedColor::Red => Color::Indexed(1),
                NamedColor::Green => Color::Indexed(2),
                NamedColor::Yellow => Color::Indexed(3),
                NamedColor::Blue => Color::Indexed(4),
                NamedColor::Magenta => Color::Indexed(5),
                NamedColor::Cyan => Color::Indexed(6),
                NamedColor::White => Color::Indexed(7),
                NamedColor::BrightBlack => Color::Indexed(8),
                NamedColor::BrightRed => Color::Indexed(9),
                NamedColor::BrightGreen => Color::Indexed(10),
                NamedColor::BrightYellow => Color::Indexed(11),
                NamedColor::BrightBlue => Color::Indexed(12),
                NamedColor::BrightMagenta => Color::Indexed(13),
                NamedColor::BrightCyan => Color::Indexed(14),
                NamedColor::BrightWhite => Color::Indexed(15),
                NamedColor::DimBlack => {
                    *attrs |= attr::DIM;
                    Color::Indexed(0)
                }
                NamedColor::DimRed => {
                    *attrs |= attr::DIM;
                    Color::Indexed(1)
                }
                NamedColor::DimGreen => {
                    *attrs |= attr::DIM;
                    Color::Indexed(2)
                }
                NamedColor::DimYellow => {
                    *attrs |= attr::DIM;
                    Color::Indexed(3)
                }
                NamedColor::DimBlue => {
                    *attrs |= attr::DIM;
                    Color::Indexed(4)
                }
                NamedColor::DimMagenta => {
                    *attrs |= attr::DIM;
                    Color::Indexed(5)
                }
                NamedColor::DimCyan => {
                    *attrs |= attr::DIM;
                    Color::Indexed(6)
                }
                NamedColor::DimWhite => {
                    *attrs |= attr::DIM;
                    Color::Indexed(7)
                }
                NamedColor::DimForeground => {
                    *attrs |= attr::DIM;
                    Color::Default
                }
                NamedColor::BrightForeground => {
                    *attrs |= attr::BOLD;
                    Color::Default
                }
                _ => Color::Default,
            },
        }
    }

    /// Snapshot of what a client should draw.
    pub fn frame(&self) -> Frame {
        let cols = self.cols as usize;
        let rows = self.rows as usize;
        let content = self.term.renderable_content();
        let display_offset = content.display_offset;
        let mut cells = vec![Cell { c: ' ', fg: Color::Default, bg: Color::Default, attrs: 0 }; cols * rows];
        for ic in content.display_iter {
            let Some(vp) = term::point_to_viewport(display_offset, ic.point) else { continue };
            let (r, c) = (vp.line, vp.column.0);
            if r >= rows || c >= cols {
                continue;
            }
            let cell = ic.cell;
            let mut a = 0u8;
            let f = cell.flags;
            if f.contains(Flags::BOLD) {
                a |= attr::BOLD;
            }
            if f.contains(Flags::DIM) {
                a |= attr::DIM;
            }
            if f.contains(Flags::ITALIC) {
                a |= attr::ITALIC;
            }
            if f.intersects(Flags::ALL_UNDERLINES) {
                a |= attr::UNDERLINE;
            }
            if f.contains(Flags::INVERSE) {
                a |= attr::INVERSE;
            }
            if f.contains(Flags::STRIKEOUT) {
                a |= attr::STRIKE;
            }
            if f.contains(Flags::HIDDEN) {
                a |= attr::HIDDEN;
            }
            if f.contains(Flags::WIDE_CHAR_SPACER) || f.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                a |= attr::WIDE_SPACER;
            }
            let fg = Self::color(cell.fg, &mut a);
            let bg = Self::color(cell.bg, &mut a);
            cells[r * cols + c] = Cell { c: cell.c, fg, bg, attrs: a };
        }
        let cursor_visible = content.mode.contains(TermMode::SHOW_CURSOR)
            && content.cursor.shape != alacritty_terminal::vte::ansi::CursorShape::Hidden
            && self.exited_at.is_none();
        let cursor = term::point_to_viewport(display_offset, content.cursor.point).map(|p| (p.column.0 as u16, p.line as u16));
        Frame {
            agent: self.name.clone(),
            cols: self.cols,
            rows: self.rows,
            cells,
            cursor,
            cursor_visible,
            scroll_offset: display_offset,
            mode: Self::mode_bits(content.mode),
        }
    }

    pub fn info(&self, pending: u32) -> proto::AgentInfo {
        proto::AgentInfo {
            name: self.name.clone(),
            harness: self.harness.clone(),
            cwd: self.cwd.display().to_string(),
            status: self.status.clone(),
            task: self.task.clone(),
            created_at: self.created_at,
            exited_at: self.exited_at,
            pending,
        }
    }
}

// Silence unused-import warnings for items used only in some cfgs.
#[allow(dead_code)]
fn _unused(_p: Point) {}
