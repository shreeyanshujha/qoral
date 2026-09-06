//! Wire protocol between clients and the daemon: length-prefixed MessagePack frames.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const MAX_FRAME: u32 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnReq {
    pub harness: String,
    pub name: Option<String>,
    pub cwd: Option<String>,
    pub prompt: Option<String>,
    pub focus: bool,
    /// Internal: run this argv instead of a harness (harness becomes "cmd"). Used for editor/debate windows.
    #[serde(default)]
    pub command: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello { client: String },
    /// Show this agent (None = nothing / home screen). Frames follow for it.
    Subscribe { agent: Option<String> },
    /// Size of the client's agent viewport; the daemon resizes the shown agent's PTY to match.
    Resize { cols: u16, rows: u16 },
    Input { agent: String, data: Vec<u8> },
    /// Scroll the agent's scrollback view. delta > 0 = older.
    Scroll { agent: String, delta: i32 },
    Spawn(SpawnReq),
    Kill { name: String },
    Focus { name: String },
    /// Write a session note for a running agent now (snapshot; the agent keeps running).
    Document { name: String },
    /// The agent's visible screen as plain text.
    Screen { name: String },
    ListAgents,
    Shutdown,
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

pub mod attr {
    pub const BOLD: u8 = 1;
    pub const DIM: u8 = 2;
    pub const ITALIC: u8 = 4;
    pub const UNDERLINE: u8 = 8;
    pub const INVERSE: u8 = 16;
    pub const STRIKE: u8 = 32;
    pub const HIDDEN: u8 = 64;
    /// This cell is the right half of a wide character; render nothing.
    pub const WIDE_SPACER: u8 = 128;
}

pub mod mode {
    pub const APP_CURSOR: u32 = 1;
    pub const APP_KEYPAD: u32 = 2;
    pub const BRACKETED_PASTE: u32 = 4;
    pub const MOUSE: u32 = 8;
    pub const SGR_MOUSE: u32 = 16;
    pub const ALT_SCREEN: u32 = 32;
    pub const MOUSE_MOTION: u32 = 64;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub agent: String,
    pub cols: u16,
    pub rows: u16,
    /// rows * cols cells, row-major.
    pub cells: Vec<Cell>,
    pub cursor: Option<(u16, u16)>,
    pub cursor_visible: bool,
    pub scroll_offset: usize,
    pub mode: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    pub harness: String,
    pub cwd: String,
    pub status: String,
    pub task: Option<String>,
    pub created_at: i64,
    pub exited_at: Option<i64>,
    pub pending: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaemonMsg {
    Hello { version: String, pid: u32 },
    Agents(Vec<AgentInfo>),
    Frame(Frame),
    Spawned { name: String, notices: Vec<String> },
    Focused { name: String },
    Error { message: String },
    Text { text: String },
    Ok,
    Pong,
    Shutdown,
}

pub async fn write_msg<W: AsyncWriteExt + Unpin, T: Serialize>(w: &mut W, msg: &T) -> Result<()> {
    let body = rmp_serde::to_vec_named(msg)?;
    let len = body.len() as u32;
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(&body).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_msg<R: AsyncReadExt + Unpin, T: for<'de> Deserialize<'de>>(r: &mut R) -> Result<T> {
    let mut lenb = [0u8; 4];
    r.read_exact(&mut lenb).await?;
    let len = u32::from_be_bytes(lenb);
    if len > MAX_FRAME {
        bail!("frame too large: {len}");
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    Ok(rmp_serde::from_slice(&body)?)
}
