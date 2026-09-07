//! Themes: semantic roles → colors and glyphs. Resolution: QORAL_THEME > config.json > default.
use ratatui::style::Color;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::paths;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ThemeFile {
    pub colors: HashMap<String, serde_json::Value>,
    pub glyphs: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct Palette {
    pub accent: Color,
    pub idle: Color,
    pub working: Color,
    pub attention: Color,
    pub documenting: Color,
    pub moderating: Color,
    pub exited: Color,
    pub starting: Color,
    pub text: Color,
    pub dim: Color,
    pub border: Color,
    #[allow(dead_code)]
    pub selection: Color,
    pub mail: Color,
    pub claude: Color,
    pub codex: Color,
    pub agy: Color,
    pub gemini: Color,
    pub opencode: Color,
    pub moderator: Color,
    pub bar_fg: Color,
    pub bar_bg: Color,
    pub bar_key: Color,
}

#[derive(Debug, Clone)]
pub struct Glyphs {
    pub idle: String,
    pub working: Vec<String>,
    pub attention: String,
    pub documenting: String,
    pub moderating: String,
    pub exited: String,
    pub starting: String,
    pub cursor: String,
    pub unread: String,
    pub mail: String,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub palette: Palette,
    pub glyphs: Glyphs,
}

pub const BUILTIN: &[&str] = &["default", "mono", "nord", "gruvbox", "dracula"];

fn builtin(name: &str) -> Option<HashMap<&'static str, &'static str>> {
    let m: &[(&str, &str)] = match name {
        "default" => &[
            ("accent", "214"), ("idle", "green"), ("working", "yellow"), ("attention", "red"), ("documenting", "cyan"), ("moderating", "cyan"),
            ("exited", "gray"), ("starting", "gray"), ("text", "white"), ("dim", "gray"), ("border", "238"), ("selection", "214"), ("mail", "magenta"),
            ("claude", "214"), ("codex", "green"), ("agy", "magenta"), ("gemini", "blue"), ("opencode", "brightcyan"), ("moderator", "cyan"),
            ("bar_fg", "250"), ("bar_bg", "236"), ("bar_key", "245"),
        ],
        "mono" => &[
            ("accent", "white"), ("idle", "white"), ("working", "245"), ("attention", "brightwhite"), ("documenting", "245"), ("moderating", "245"),
            ("exited", "240"), ("starting", "240"), ("text", "white"), ("dim", "240"), ("border", "238"), ("selection", "white"), ("mail", "white"),
            ("claude", "white"), ("codex", "250"), ("agy", "245"), ("gemini", "250"), ("opencode", "250"), ("moderator", "245"), ("bar_fg", "250"), ("bar_bg", "235"), ("bar_key", "245"),
        ],
        "nord" => &[
            ("accent", "#88c0d0"), ("idle", "#a3be8c"), ("working", "#ebcb8b"), ("attention", "#bf616a"), ("documenting", "#b48ead"), ("moderating", "#81a1c1"),
            ("exited", "#4c566a"), ("starting", "#4c566a"), ("text", "#eceff4"), ("dim", "#616e88"), ("border", "#3b4252"), ("selection", "#88c0d0"), ("mail", "#b48ead"),
            ("claude", "#d08770"), ("codex", "#a3be8c"), ("agy", "#b48ead"), ("gemini", "#81a1c1"), ("opencode", "#8fbcbb"), ("moderator", "#88c0d0"), ("bar_fg", "#d8dee9"), ("bar_bg", "#2e3440"), ("bar_key", "#616e88"),
        ],
        "gruvbox" => &[
            ("accent", "#fabd2f"), ("idle", "#b8bb26"), ("working", "#fabd2f"), ("attention", "#fb4934"), ("documenting", "#d3869b"), ("moderating", "#83a598"),
            ("exited", "#665c54"), ("starting", "#665c54"), ("text", "#ebdbb2"), ("dim", "#928374"), ("border", "#3c3836"), ("selection", "#fabd2f"), ("mail", "#d3869b"),
            ("claude", "#fe8019"), ("codex", "#b8bb26"), ("agy", "#d3869b"), ("gemini", "#83a598"), ("opencode", "#8ec07c"), ("moderator", "#8ec07c"), ("bar_fg", "#ebdbb2"), ("bar_bg", "#282828"), ("bar_key", "#928374"),
        ],
        "dracula" => &[
            ("accent", "#bd93f9"), ("idle", "#50fa7b"), ("working", "#f1fa8c"), ("attention", "#ff5555"), ("documenting", "#ff79c6"), ("moderating", "#8be9fd"),
            ("exited", "#6272a4"), ("starting", "#6272a4"), ("text", "#f8f8f2"), ("dim", "#6272a4"), ("border", "#44475a"), ("selection", "#bd93f9"), ("mail", "#ff79c6"),
            ("claude", "#ffb86c"), ("codex", "#50fa7b"), ("agy", "#ff79c6"), ("gemini", "#8be9fd"), ("opencode", "#50fa7b"), ("moderator", "#8be9fd"), ("bar_fg", "#f8f8f2"), ("bar_bg", "#282a36"), ("bar_key", "#6272a4"),
        ],
        _ => return None,
    };
    Some(m.iter().cloned().collect())
}

/// Parse "red" | "brightblue" | 214 | "colour214" | "#rrggbb" into a ratatui Color.
pub fn parse_color(v: &serde_json::Value) -> Option<Color> {
    if let Some(n) = v.as_u64() {
        return Some(Color::Indexed(n.min(255) as u8));
    }
    let s = v.as_str()?.trim().to_lowercase();
    if let Some(h) = s.strip_prefix('#') {
        let h = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect::<String>() } else { h.to_string() };
        if h.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&h[0..2], 16).ok()?;
        let g = u8::from_str_radix(&h[2..4], 16).ok()?;
        let b = u8::from_str_radix(&h[4..6], 16).ok()?;
        return Some(Color::Rgb(r, g, b));
    }
    let s = s.trim_start_matches("colour").trim_start_matches("color");
    if let Ok(n) = s.parse::<u16>() {
        return Some(Color::Indexed(n.min(255) as u8));
    }
    Some(match s {
        "black" => Color::Indexed(0),
        "red" => Color::Indexed(1),
        "green" => Color::Indexed(2),
        "yellow" => Color::Indexed(3),
        "blue" => Color::Indexed(4),
        "magenta" => Color::Indexed(5),
        "cyan" => Color::Indexed(6),
        "white" => Color::Indexed(7),
        "gray" | "grey" | "brightblack" => Color::Indexed(8),
        "brightred" => Color::Indexed(9),
        "brightgreen" => Color::Indexed(10),
        "brightyellow" => Color::Indexed(11),
        "brightblue" => Color::Indexed(12),
        "brightmagenta" => Color::Indexed(13),
        "brightcyan" => Color::Indexed(14),
        "brightwhite" => Color::Indexed(15),
        "default" | "reset" => Color::Reset,
        _ => return None,
    })
}

fn theme_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(c) = dirs::config_dir() {
        v.push(c.join("qoral/themes"));
    }
    v.push(paths::home().join("themes"));
    v
}

pub fn user_themes() -> HashMap<String, ThemeFile> {
    let mut out = HashMap::new();
    if let Some(c) = dirs::config_dir() {
        if let Ok(s) = std::fs::read_to_string(c.join("qoral/theme.json")) {
            if let Ok(t) = serde_json::from_str::<ThemeFile>(&s) {
                out.insert("custom".to_string(), t);
            }
        }
    }
    for d in theme_dirs() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "json").unwrap_or(false) {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    if let Ok(t) = serde_json::from_str::<ThemeFile>(&s) {
                        out.insert(p.file_stem().unwrap().to_string_lossy().to_string(), t);
                    }
                }
            }
        }
    }
    out
}

pub fn list_themes() -> Vec<String> {
    let mut v: Vec<String> = BUILTIN.iter().map(|s| s.to_string()).collect();
    for k in user_themes().keys() {
        if !v.contains(k) {
            v.push(k.clone());
        }
    }
    v.sort();
    v
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ConfigTheme {
    theme: Option<String>,
    colors: HashMap<String, serde_json::Value>,
    glyphs: HashMap<String, serde_json::Value>,
}

pub fn load() -> Theme {
    let cfg: ConfigTheme = std::fs::read_to_string(paths::home().join("config.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let wanted = std::env::var("QORAL_THEME").ok().or(cfg.theme).unwrap_or_else(|| "default".into());
    let users = user_themes();
    let (name, named_colors, named_glyphs): (String, HashMap<String, serde_json::Value>, HashMap<String, serde_json::Value>) = if let Some(u) = users.get(&wanted) {
        (wanted.clone(), u.colors.clone(), u.glyphs.clone())
    } else if let Some(b) = builtin(&wanted) {
        (wanted.clone(), b.into_iter().map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string()))).collect(), HashMap::new())
    } else {
        ("default".into(), HashMap::new(), HashMap::new())
    };
    let defaults = builtin("default").unwrap();
    let get = |role: &str| -> Color {
        cfg.colors
            .get(role)
            .and_then(parse_color)
            .or_else(|| named_colors.get(role).and_then(parse_color))
            .or_else(|| defaults.get(role).and_then(|v| parse_color(&serde_json::Value::String(v.to_string()))))
            .unwrap_or(Color::Reset)
    };
    let palette = Palette {
        accent: get("accent"),
        idle: get("idle"),
        working: get("working"),
        attention: get("attention"),
        documenting: get("documenting"),
        moderating: get("moderating"),
        exited: get("exited"),
        starting: get("starting"),
        text: get("text"),
        dim: get("dim"),
        border: get("border"),
        selection: get("selection"),
        mail: get("mail"),
        claude: get("claude"),
        codex: get("codex"),
        agy: get("agy"),
        gemini: get("gemini"),
        opencode: get("opencode"),
        moderator: get("moderator"),
        bar_fg: get("bar_fg"),
        bar_bg: get("bar_bg"),
        bar_key: get("bar_key"),
    };
    let g = |k: &str, d: &str| -> String {
        cfg.glyphs.get(k).or_else(|| named_glyphs.get(k)).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| d.to_string())
    };
    let spinner = cfg
        .glyphs
        .get("working")
        .or_else(|| named_glyphs.get("working"))
        .and_then(|v| match v {
            serde_json::Value::Array(a) => Some(a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>()),
            serde_json::Value::String(s) => Some(vec![s.clone()]),
            _ => None,
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["◐".into(), "◓".into(), "◑".into(), "◒".into()]);
    let glyphs = Glyphs {
        idle: g("idle", "●"),
        working: spinner,
        attention: g("attention", "!"),
        documenting: g("documenting", "✎"),
        moderating: g("moderating", "⚖"),
        exited: g("exited", "○"),
        starting: g("starting", "◌"),
        cursor: g("cursor", "▸"),
        unread: g("unread", "↓"),
        mail: g("mail", "✉"),
    };
    Theme { name, palette, glyphs }
}

pub fn harness_color(p: &Palette, h: &str) -> Color {
    match h {
        "claude" => p.claude,
        "codex" => p.codex,
        "agy" => p.agy,
        "gemini" => p.gemini,
        "opencode" => p.opencode,
        "moderator" => p.moderator,
        _ => p.text,
    }
}

/// Write a starter theme file the user can edit.
pub fn init_theme(name: &str, from: Option<&str>, force: bool) -> anyhow::Result<PathBuf> {
    let base = from.and_then(builtin).or_else(|| builtin("default")).unwrap();
    let dir = paths::home().join("themes");
    std::fs::create_dir_all(&dir)?;
    let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    let safe = if safe.is_empty() { "custom".to_string() } else { safe };
    let file = dir.join(format!("{safe}.json"));
    if file.exists() && !force {
        anyhow::bail!("{} exists (use --force to overwrite)", file.display());
    }
    let mut colors = serde_json::Map::new();
    for (k, v) in base {
        colors.insert(k.to_string(), serde_json::Value::String(v.to_string()));
    }
    let doc = serde_json::json!({ "colors": colors, "glyphs": { "idle": "●", "attention": "!", "working": ["◐", "◓", "◑", "◒"] } });
    std::fs::write(&file, format!("{}\n", serde_json::to_string_pretty(&doc)?))?;
    Ok(file)
}

pub fn describe() -> String {
    let t = load();
    let sw = |c: Color| -> String {
        match c {
            Color::Indexed(i) => format!("\x1b[38;5;{i}m██\x1b[0m"),
            Color::Rgb(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m██\x1b[0m"),
            _ => "██".into(),
        }
    };
    let p = &t.palette;
    let mut out = vec![format!("themes: {}", list_themes().join(", ")), format!("active: {}", t.name), String::new()];
    out.push([("accent", p.accent), ("idle", p.idle), ("working", p.working), ("attention", p.attention), ("documenting", p.documenting), ("exited", p.exited)].iter().map(|(n, c)| format!("{} {n}", sw(*c))).collect::<Vec<_>>().join("   "));
    out.push([("claude", p.claude), ("codex", p.codex), ("agy", p.agy), ("gemini", p.gemini), ("opencode", p.opencode), ("moderator", p.moderator)].iter().map(|(n, c)| format!("{} {n}", sw(*c))).collect::<Vec<_>>().join("   "));
    let g = &t.glyphs;
    out.push(format!("glyphs: idle {}  working {}  attention {}  documenting {}  exited {}", g.idle, g.working.join(""), g.attention, g.documenting, g.exited));
    out.join("\n")
}
