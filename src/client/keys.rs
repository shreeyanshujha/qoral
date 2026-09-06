//! Translate crossterm key/mouse events into the bytes a terminal application expects.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::proto::mode;

fn modifier_param(m: KeyModifiers) -> u8 {
    let mut n = 1;
    if m.contains(KeyModifiers::SHIFT) {
        n += 1;
    }
    if m.contains(KeyModifiers::ALT) {
        n += 2;
    }
    if m.contains(KeyModifiers::CONTROL) {
        n += 4;
    }
    n
}

fn csi(final_byte: char, m: KeyModifiers, app_cursor: bool) -> Vec<u8> {
    let p = modifier_param(m);
    if p == 1 {
        if app_cursor {
            format!("\x1bO{final_byte}").into_bytes()
        } else {
            format!("\x1b[{final_byte}").into_bytes()
        }
    } else {
        format!("\x1b[1;{p}{final_byte}").into_bytes()
    }
}

fn tilde(n: u8, m: KeyModifiers) -> Vec<u8> {
    let p = modifier_param(m);
    if p == 1 {
        format!("\x1b[{n}~").into_bytes()
    } else {
        format!("\x1b[{n};{p}~").into_bytes()
    }
}

/// Encode a key press for the terminal. `term_mode` is the agent's current mode bits.
pub fn encode_key(ev: &KeyEvent, term_mode: u32) -> Option<Vec<u8>> {
    let m = ev.modifiers;
    let app_cursor = term_mode & mode::APP_CURSOR != 0;
    let alt = m.contains(KeyModifiers::ALT);
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let mut out: Vec<u8> = Vec::new();
    match ev.code {
        KeyCode::Char(c) => {
            if ctrl {
                let b: Option<u8> = match c.to_ascii_lowercase() {
                    'a'..='z' => Some(c.to_ascii_lowercase() as u8 - b'a' + 1),
                    ' ' | '@' | '2' => Some(0),
                    '[' | '3' => Some(0x1b),
                    '\\' | '4' => Some(0x1c),
                    ']' | '5' => Some(0x1d),
                    '^' | '6' => Some(0x1e),
                    '_' | '7' | '/' => Some(0x1f),
                    '8' | '?' => Some(0x7f),
                    _ => None,
                };
                if let Some(b) = b {
                    if alt {
                        out.push(0x1b);
                    }
                    out.push(b);
                    return Some(out);
                }
                return None;
            }
            if alt {
                out.push(0x1b);
            }
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        KeyCode::Enter => {
            if alt {
                out.push(0x1b);
            }
            out.push(b'\r');
        }
        KeyCode::Tab => out.push(b'\t'),
        KeyCode::BackTab => out.extend_from_slice(b"\x1b[Z"),
        KeyCode::Backspace => {
            if alt {
                out.push(0x1b);
            }
            out.push(if ctrl { 0x08 } else { 0x7f });
        }
        KeyCode::Esc => out.push(0x1b),
        KeyCode::Up => out = csi('A', m, app_cursor),
        KeyCode::Down => out = csi('B', m, app_cursor),
        KeyCode::Right => out = csi('C', m, app_cursor),
        KeyCode::Left => out = csi('D', m, app_cursor),
        KeyCode::Home => out = csi('H', m, app_cursor),
        KeyCode::End => out = csi('F', m, app_cursor),
        KeyCode::Insert => out = tilde(2, m),
        KeyCode::Delete => out = tilde(3, m),
        KeyCode::PageUp => out = tilde(5, m),
        KeyCode::PageDown => out = tilde(6, m),
        KeyCode::F(n) => {
            let p = modifier_param(m);
            out = match n {
                1..=4 => {
                    let f = [b'P', b'Q', b'R', b'S'][(n - 1) as usize] as char;
                    if p == 1 {
                        format!("\x1bO{f}").into_bytes()
                    } else {
                        format!("\x1b[1;{p}{f}").into_bytes()
                    }
                }
                5 => tilde(15, m),
                6 => tilde(17, m),
                7 => tilde(18, m),
                8 => tilde(19, m),
                9 => tilde(20, m),
                10 => tilde(21, m),
                11 => tilde(23, m),
                12 => tilde(24, m),
                _ => return None,
            };
        }
        KeyCode::Null => out.push(0),
        _ => return None,
    }
    Some(out)
}

pub fn encode_paste(text: &str, term_mode: u32) -> Vec<u8> {
    let mut clean = text.replace("\r\n", "\r").replace('\n', "\r");
    if term_mode & mode::BRACKETED_PASTE != 0 {
        clean = format!("\x1b[200~{clean}\x1b[201~");
    }
    clean.into_bytes()
}

/// SGR mouse encoding relative to the agent viewport origin. Returns None if the app didn't ask for mouse.
pub fn encode_mouse(ev: &MouseEvent, origin: (u16, u16), term_mode: u32) -> Option<Vec<u8>> {
    if term_mode & mode::MOUSE == 0 {
        return None;
    }
    let x = ev.column.checked_sub(origin.0)? as u32 + 1;
    let y = ev.row.checked_sub(origin.1)? as u32 + 1;
    let mut mods = 0;
    if ev.modifiers.contains(KeyModifiers::SHIFT) {
        mods |= 4;
    }
    if ev.modifiers.contains(KeyModifiers::ALT) {
        mods |= 8;
    }
    if ev.modifiers.contains(KeyModifiers::CONTROL) {
        mods |= 16;
    }
    let button = |b: MouseButton| match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let (code, release) = match ev.kind {
        MouseEventKind::Down(b) => (button(b), false),
        MouseEventKind::Up(b) => (button(b), true),
        MouseEventKind::Drag(b) => (button(b) + 32, false),
        MouseEventKind::Moved => {
            if term_mode & mode::MOUSE_MOTION == 0 {
                return None;
            }
            (35, false)
        }
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        MouseEventKind::ScrollLeft => (66, false),
        MouseEventKind::ScrollRight => (67, false),
    };
    let cb = code | mods;
    Some(format!("\x1b[<{cb};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes())
}
