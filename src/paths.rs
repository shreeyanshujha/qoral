//! Filesystem locations shared by daemon, client and CLI.
use std::path::{Path, PathBuf};

pub const HARNESSES: &[&str] = &["claude", "codex", "agy", "gemini"];

pub fn home() -> PathBuf {
    if let Ok(p) = std::env::var("QORAL_HOME") {
        return PathBuf::from(p);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".local/share"))
        .join("qoral")
}

pub fn db_path() -> PathBuf {
    home().join("qoral.db")
}

pub fn agents_dir() -> PathBuf {
    home().join("agents")
}

pub fn agent_dir(name: &str) -> PathBuf {
    let d = agents_dir().join(name);
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn daemon_log() -> PathBuf {
    home().join("daemon.log")
}

/// Unix socket the daemon listens on. Kept short (sockaddr_un caps paths at ~108 bytes):
/// $XDG_RUNTIME_DIR/qoral/<id>.sock, else /tmp/qoral-<uid>/<id>.sock, where <id> is "daemon" for the
/// default home and a hash of QORAL_HOME otherwise. QORAL_SOCKET=<absolute path> overrides everything.
pub fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("QORAL_SOCKET") {
        let pb = PathBuf::from(&p);
        if pb.is_absolute() {
            return pb;
        }
        return runtime_dir().join(format!("{p}.sock"));
    }
    let id = match std::env::var("QORAL_HOME") {
        Ok(h) => format!("daemon-{:08x}", fnv1a(h.as_bytes())),
        Err(_) => "daemon".to_string(),
    };
    runtime_dir().join(format!("{id}.sock"))
}

fn runtime_dir() -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .map(|p| p.join("qoral"))
        .unwrap_or_else(|| std::env::temp_dir().join(format!("qoral-{}", uid())));
    let _ = std::fs::create_dir_all(&base);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700));
    }
    base
}

fn uid() -> u32 {
    #[cfg(unix)]
    {
        // no libc dependency: read from /proc when available, else fall back to $UID
        if let Ok(s) = std::fs::read_to_string("/proc/self/status") {
            if let Some(l) = s.lines().find(|l| l.starts_with("Uid:")) {
                if let Some(u) = l.split_whitespace().nth(1).and_then(|x| x.parse().ok()) {
                    return u;
                }
            }
        }
    }
    std::env::var("UID").ok().and_then(|u| u.parse().ok()).unwrap_or(0)
}

fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

pub fn ensure_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(agents_dir())
}

pub fn expand_home(p: &str) -> PathBuf {
    if p == "~" {
        return dirs::home_dir().unwrap_or_default();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return dirs::home_dir().unwrap_or_default().join(rest);
    }
    PathBuf::from(p)
}

pub fn short_dir(p: &Path) -> String {
    let s = p.display().to_string();
    if let Some(h) = dirs::home_dir() {
        let hs = h.display().to_string();
        if let Some(rest) = s.strip_prefix(&hs) {
            return format!("~{rest}");
        }
    }
    s
}
