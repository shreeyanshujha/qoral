//! How each agent CLI is launched and wired to the qoral MCP server.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Launch {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub notices: Vec<String>,
    pub session_id: Option<String>,
}

pub fn intro(name: &str, harness: &str) -> String {
    format!(
        "You are running inside qoral, a shared terminal workspace where several AI coding agents work side by side and can talk to each other.\n\
Your agent name is \"{name}\" (harness: {harness}). Other agents, and the human operator (addressed as \"human\"), are reachable through the `qoral` MCP tools:\n\
- qoral_list_agents: who else is running, what they are working on, and whether they are idle.\n\
- qoral_send(to, message): send a message to another agent by name, to \"all\" to broadcast, or to \"human\" to reach the person.\n\
- qoral_inbox(): fetch messages addressed to you that you have not read yet.\n\
- qoral_wait(timeout_seconds): block until a message arrives. Use it right after asking another agent something so you get the answer.\n\
- qoral_spawn(harness, name, cwd, prompt): start a new agent and delegate work to it.\n\
- qoral_log(limit): recent traffic on the bus, for context.\n\
- qoral_remember(text): save a durable fact about this project for every future agent (goes into .qoral/KNOWLEDGE.md).\n\
- qoral_knowledge(): read the project's accumulated knowledge and the list of past session notes.\n\
Messages may also be delivered straight into your prompt as a line starting with \"[qoral] message from X:\". Treat those exactly like a request from X: act on it and reply with qoral_send(to=\"X\") rather than answering in plain text, because X cannot see your screen.\n\
Recipients do not share your context, so make every message self-contained: what you need, why, and the relevant file paths or snippets. Keep it concise.\n\
Coordinate before editing files another agent is likely touching. When you finish a piece of delegated work, report back to whoever asked. If you have no task yet, say hello briefly and wait for instructions."
    )
}

/// The project knowledge section appended to every agent's briefing.
pub fn knowledge_section(cwd: &Path) -> String {
    let kdir = cwd.join(".qoral");
    let know = std::fs::read_to_string(kdir.join("KNOWLEDGE.md")).unwrap_or_default();
    let know = know.trim();
    let mut out = format!("## Project knowledge (accumulated by earlier qoral sessions in {})\n", cwd.display());
    if know.is_empty() {
        out.push_str("(none yet — you are among the first agents in this project)\n");
    } else if know.len() > 8000 {
        let mut cut = 8000;
        while !know.is_char_boundary(cut) {
            cut -= 1;
        }
        out.push_str(&know[..cut]);
        out.push_str("\n…(truncated; read .qoral/KNOWLEDGE.md for the rest)\n");
    } else {
        out.push_str(know);
        out.push('\n');
    }
    // recent notes
    if let Ok(rd) = std::fs::read_dir(kdir.join("notes")) {
        let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().map(|e| e == "md").unwrap_or(false)).collect();
        files.sort();
        files.reverse();
        if !files.is_empty() {
            out.push_str("\nRecent session notes (read them with your file tools when relevant):\n");
            for f in files.iter().take(8) {
                let title = std::fs::read_to_string(f)
                    .ok()
                    .and_then(|t| t.lines().find(|l| l.starts_with("# ")).map(|l| l[2..].trim().to_string()))
                    .unwrap_or_else(|| f.file_name().unwrap().to_string_lossy().to_string());
                out.push_str(&format!("- .qoral/notes/{} — {title}\n", f.file_name().unwrap().to_string_lossy()));
            }
        }
    }
    out.push_str("\nRecord durable facts for future agents with qoral_remember(text): architecture, conventions, decisions, gotchas, how to run/test. When your session ends it is summarized automatically into .qoral/notes and merged into .qoral/KNOWLEDGE.md.");
    out
}

fn self_exe() -> String {
    std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "qoral".into())
}

pub fn build_launch(harness: &str, name: &str, cwd: &Path, prompt: Option<&str>) -> Result<Launch> {
    let dir = paths::agent_dir(name);
    let exe = self_exe();
    let mcp_args = vec!["mcp".to_string(), "--agent".to_string(), name.to_string()];
    let mcp_json = serde_json::json!({ "mcpServers": { "qoral": { "command": exe, "args": mcp_args } } });
    let system = format!("{}\n\n{}", intro(name, harness), knowledge_section(cwd));
    let full_prompt = match prompt {
        Some(p) => format!("{system}\n\nYour task:\n{p}"),
        None => system.clone(),
    };
    let mut notices = Vec::new();

    match harness {
        "claude" => {
            let cfg = dir.join("mcp.json");
            std::fs::write(&cfg, serde_json::to_string_pretty(&mcp_json)?)?;
            let session_id = uuid::Uuid::new_v4().to_string();
            let mut argv = vec![
                "claude".into(),
                "--name".into(),
                name.into(),
                "--session-id".into(),
                session_id.clone(),
                "--mcp-config".into(),
                cfg.display().to_string(),
                "--allowedTools".into(),
                "mcp__qoral".into(),
                "--append-system-prompt".into(),
                system,
            ];
            if let Some(p) = prompt {
                argv.push(p.into());
            }
            Ok(Launch { argv, env: vec![], notices, session_id: Some(session_id) })
        }
        "codex" => {
            let argv = vec![
                "codex".into(),
                "-c".into(),
                format!("mcp_servers.qoral.command={}", serde_json::to_string(&exe)?),
                "-c".into(),
                format!("mcp_servers.qoral.args={}", serde_json::to_string(&mcp_args)?),
                full_prompt,
            ];
            Ok(Launch { argv, env: vec![], notices, session_id: None })
        }
        "agy" => {
            notices.extend(ensure_agy_integration(&exe)?);
            Ok(Launch { argv: vec!["agy".into(), "-i".into(), full_prompt], env: vec![], notices, session_id: None })
        }
        "gemini" => {
            let cfg = dir.join("gemini-settings.json");
            let mut j = mcp_json.clone();
            j["mcpServers"]["qoral"]["trust"] = serde_json::Value::Bool(true);
            std::fs::write(&cfg, serde_json::to_string_pretty(&j)?)?;
            Ok(Launch {
                argv: vec!["gemini".into(), "-i".into(), full_prompt],
                env: vec![("GEMINI_CLI_SYSTEM_SETTINGS_PATH".into(), cfg.display().to_string())],
                notices,
                session_id: None,
            })
        }
        other => bail!("unknown harness \"{other}\" (claude | codex | agy | gemini)"),
    }
}

pub const TOOL_NAMES: &[&str] = &[
    "qoral_whoami",
    "qoral_list_agents",
    "qoral_send",
    "qoral_inbox",
    "qoral_wait",
    "qoral_spawn",
    "qoral_log",
    "qoral_remember",
    "qoral_knowledge",
];

/// Antigravity has no per-session MCP flag: register globally + pre-approve tools. Idempotent.
pub fn ensure_agy_integration(exe: &str) -> Result<Vec<String>> {
    let home = dirs::home_dir().unwrap_or_default();
    let mcp_file = home.join(".gemini/config/mcp_config.json");
    let settings_file = home.join(".gemini/antigravity-cli/settings.json");
    let mut notices = Vec::new();

    let mut mcp: serde_json::Value = std::fs::read_to_string(&mcp_file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({}));
    if !mcp.is_object() {
        mcp = serde_json::json!({});
    }
    if mcp.get("mcpServers").map(|v| !v.is_object()).unwrap_or(true) {
        mcp["mcpServers"] = serde_json::json!({});
    }
    let want = serde_json::json!({ "command": exe, "args": ["mcp"], "disabled": false });
    let have = mcp["mcpServers"].get("qoral").cloned();
    let needs = match &have {
        Some(h) => h.get("command") != want.get("command") || h.get("args") != want.get("args") || h.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false),
        None => true,
    };
    if needs {
        let mut merged = have.unwrap_or(serde_json::json!({}));
        for (k, v) in want.as_object().unwrap() {
            merged[k] = v.clone();
        }
        mcp["mcpServers"]["qoral"] = merged;
        if let Some(p) = mcp_file.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(&mcp_file, format!("{}\n", serde_json::to_string_pretty(&mcp)?))?;
        notices.push(format!("registered the qoral MCP server for Antigravity in {} (remove with: agy mcp remove qoral)", mcp_file.display()));
    }

    let mut settings: serde_json::Value = std::fs::read_to_string(&settings_file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({}));
    if !settings.is_object() {
        settings = serde_json::json!({});
    }
    if settings.get("permissions").map(|v| !v.is_object()).unwrap_or(true) {
        settings["permissions"] = serde_json::json!({});
    }
    if settings["permissions"].get("allow").map(|v| !v.is_array()).unwrap_or(true) {
        settings["permissions"]["allow"] = serde_json::json!([]);
    }
    let allow = settings["permissions"]["allow"].as_array_mut().unwrap();
    let mut added = 0;
    for t in TOOL_NAMES {
        let rule = serde_json::Value::String(format!("mcp(qoral/{t})"));
        if !allow.contains(&rule) {
            allow.push(rule);
            added += 1;
        }
    }
    if added > 0 {
        if let Some(p) = settings_file.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(&settings_file, format!("{}\n", serde_json::to_string_pretty(&settings)?))?;
        notices.push(format!("added {added} mcp(qoral/*) allow rules to {} so agents aren't prompted for bus calls", settings_file.display()));
    }
    Ok(notices)
}

const NAMES: &[&str] = &["ada", "grace", "linus", "alan", "dennis", "ken", "margaret", "barbara", "edsger", "donald", "radia", "hedy", "tim", "guido", "brendan", "anders"];

pub fn suggest_name(taken: &[String]) -> String {
    NAMES
        .iter()
        .find(|n| !taken.iter().any(|t| t == *n))
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("agent-{:x}", crate::db::now_ms() % 0xffff))
}

pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    name.len() <= 32
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !["all", "human", "you", "user", "everyone", "operator"].contains(&name)
}
