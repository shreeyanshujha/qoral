//! `qoral doctor`: environment checks with platform-specific advice.
use crate::{ctl, knowledge, paths, theme};

const G: &str = "\x1b[32m";
const Y: &str = "\x1b[33m";
const R: &str = "\x1b[31m";
const D: &str = "\x1b[2m";
const B: &str = "\x1b[1m";
const X: &str = "\x1b[0m";

fn ok(s: &str) -> String {
    format!("{G}✔{X} {s}")
}
fn warn(s: &str) -> String {
    format!("{Y}!{X} {s}")
}
fn bad(s: &str) -> String {
    format!("{R}✘{X} {s}")
}

pub fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if std::fs::read_to_string("/proc/version").map(|s| s.to_lowercase().contains("microsoft")).unwrap_or(false) {
        "Windows (WSL2)"
    } else {
        "Linux"
    }
}

pub async fn run() -> i32 {
    let mut lines: Vec<String> = Vec::new();
    let mut problems = 0;
    let p = platform_name();
    lines.push(format!("{B}qoral {}{X} · {p} · {}", env!("CARGO_PKG_VERSION"), std::env::current_exe().map(|e| e.display().to_string()).unwrap_or_default()));
    lines.push(String::new());

    // terminal
    let term = std::env::var("TERM").unwrap_or_else(|_| "(unset)".into());
    let colorterm = std::env::var("COLORTERM").ok();
    let truecolor = matches!(colorterm.as_deref(), Some("truecolor") | Some("24bit"));
    let prog = std::env::var("TERM_PROGRAM").ok().map(|s| format!(" ({s})")).or_else(|| std::env::var("WT_SESSION").ok().map(|_| " (Windows Terminal)".into())).unwrap_or_default();
    lines.push(ok(&format!("terminal: TERM={term}{}{prog}", colorterm.as_deref().map(|c| format!(" COLORTERM={c}")).unwrap_or_default())));
    let t = theme::load();
    lines.push(if truecolor { ok(&format!("theme: {} (truecolor available; hex themes render exactly)", t.name)) } else { warn(&format!("theme: {} (no truecolor advertised; hex themes may approximate)", t.name)) });
    lines.push(format!("{D}  available: {} · preview with `qoral theme`{X}", theme::list_themes().join(", ")));

    // daemon
    lines.push(String::new());
    lines.push(format!("{B}daemon{X}"));
    match ctl::try_connect().await {
        Some(_) => lines.push(ok(&format!("running · socket {}", paths::socket_path().display()))),
        None => lines.push(format!("{D}  not running (starts automatically with `qoral`) · socket {}{X}", paths::socket_path().display())),
    }

    // agent CLIs
    lines.push(String::new());
    lines.push(format!("{B}agent CLIs{X}"));
    let labels = [("claude", "Claude Code"), ("codex", "OpenAI Codex"), ("agy", "Antigravity (agy)"), ("gemini", "Gemini CLI"), ("opencode", "OpenCode (any provider, e.g. DeepSeek)")];
    let mut found = 0;
    for (h, label) in labels {
        if knowledge::which(h) {
            found += 1;
            let v = std::process::Command::new(h).arg("--version").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").to_string()).unwrap_or_default();
            lines.push(ok(&format!("{h:<7} {label}  {D}{v}{X}")));
        } else {
            lines.push(warn(&format!("{h:<7} {label}  {D}not on PATH{X}")));
        }
    }
    if found == 0 {
        lines.push(bad("no agent CLIs found; install at least one (claude, codex, agy, gemini)"));
        problems += 1;
    }
    match knowledge::pick_summarizer() {
        Some(s) => lines.push(ok(&format!("session documentation via headless {s}"))),
        None => lines.push(warn("session documentation disabled (no summarizer CLI, or summarizer=none)")),
    }

    // data
    lines.push(String::new());
    lines.push(format!("{B}data{X}"));
    lines.push(ok(&format!("workspace data: {}", paths::home().display())));
    let cfg = paths::home().join("config.json");
    lines.push(if cfg.exists() { ok(&format!("config: {}", cfg.display())) } else { warn(&format!("config: {} {D}(not created; defaults in use){X}", cfg.display())) });

    // notes
    lines.push(String::new());
    lines.push(format!("{B}notes for {p}{X}"));
    match p {
        "macOS" => {
            lines.push(format!("{D}- Alt chords need Option to send Meta: Terminal.app → Settings → Profiles → Keyboard → \"Use Option as Meta key\";{X}"));
            lines.push(format!("{D}  iTerm2 → Profiles → Keys → Left Option key: Esc+. Or use the sidebar keys (n, m, D, l …) instead.{X}"));
        }
        "Windows" => {
            lines.push(format!("{D}- native Windows support is in progress (ConPTY); WSL2 works today.{X}"));
        }
        "Windows (WSL2)" => {
            lines.push(format!("{D}- Windows Terminal binds Alt+arrows to pane focus; use Alt+h / Alt+l in qoral, or unbind them.{X}"));
            lines.push(format!("{D}- Keep your projects on the Linux filesystem (~/…), not /mnt/c, for speed.{X}"));
        }
        _ => {
            lines.push(format!("{D}- Any terminal works (Foot, Alacritty, Kitty, GNOME Terminal, Konsole …).{X}"));
            lines.push(format!("{D}- If Alt chords are eaten by your window manager, use the sidebar keys instead.{X}"));
        }
    }
    lines.push(String::new());
    lines.push(if problems > 0 { bad(&format!("{problems} problem(s) to fix before qoral will run")) } else { ok("ready: run `qoral`") });
    println!("{}", lines.join("\n"));
    problems
}
