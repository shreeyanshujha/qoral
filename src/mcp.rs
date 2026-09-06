//! The MCP server agents talk through. Stdio, newline-delimited JSON-RPC 2.0, no async needed.
use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use crate::db::{self, Db};
use crate::paths;

fn tool(name: &str, desc: &str, props: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": desc,
        "inputSchema": { "type": "object", "properties": props, "required": required, "additionalProperties": false }
    })
}

fn tools() -> Vec<Value> {
    vec![
        tool("qoral_whoami", "Your own qoral identity: name, harness, working directory.", json!({}), &[]),
        tool("qoral_list_agents", "List every agent in the workspace with harness, working directory, current status (idle/working/attention/exited) and task.", json!({}), &[]),
        tool(
            "qoral_send",
            "Send a message to another agent by name, to \"all\" (broadcast to every other agent) or to \"human\" (the operator). Recipients do not share your context: be self-contained and concise. Idle recipients receive it in their prompt within seconds; busy ones get it when they next become idle or call qoral_inbox.",
            json!({ "to": { "type": "string", "description": "Agent name, \"all\", or \"human\"." }, "message": { "type": "string", "description": "The message text." } }),
            &["to", "message"],
        ),
        tool("qoral_inbox", "Fetch messages addressed to you that you have not read yet, oldest first. Marks them read.", json!({}), &[]),
        tool(
            "qoral_wait",
            "Block until a message arrives for you (or the timeout passes), then return it. Use right after asking another agent a question. Returns immediately if unread messages already exist. If it times out, you may call it again.",
            json!({ "timeout_seconds": { "type": "number", "description": "Max seconds to wait (default 120, max 600)." }, "from": { "type": "string", "description": "Only wait for messages from this sender (optional)." } }),
            &[],
        ),
        tool(
            "qoral_spawn",
            "Start a new agent in the workspace and give it a task. The new agent knows your name and can message you back; follow up with qoral_wait to receive its report.",
            json!({ "harness": { "type": "string", "enum": paths::HARNESSES, "description": "Which CLI to run." }, "name": { "type": "string", "description": "Short unique name (letters, digits, - _)." }, "cwd": { "type": "string", "description": "Working directory (defaults to yours)." }, "prompt": { "type": "string", "description": "The task to give the new agent." } }),
            &["harness", "prompt"],
        ),
        tool("qoral_log", "Recent messages on the bus between all agents (for context). Default 30.", json!({ "limit": { "type": "number" } }), &[]),
        tool(
            "qoral_remember",
            "Save a durable fact about this project for every future agent working here: architecture, conventions, a decision and its reasoning, a gotcha, how to run or test something. It is appended to .qoral/KNOWLEDGE.md immediately and injected into future agents' prompts. One fact per call; be specific and concise.",
            json!({ "text": { "type": "string", "description": "The fact to remember." } }),
            &["text"],
        ),
        tool("qoral_knowledge", "Read the project's accumulated knowledge document (.qoral/KNOWLEDGE.md) and the list of recent session notes.", json!({}), &[]),
    ]
}

fn text(s: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": s.into() }] })
}
fn error_result(s: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": s.into() }], "isError": true })
}

fn fmt_msg(m: &db::Message) -> String {
    let secs = m.ts / 1000;
    let (h, mi, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("[{h:02}:{mi:02}:{s:02} UTC] {} → {}: {}", m.sender, m.recipient, m.body)
}

const SKELETON: &str = "# Project knowledge\n\n## Overview\n\n## Layout\n\n## Conventions\n\n## Decisions\n\n## Gotchas\n\n## How to run & test\n\n## Notes from agents\n";

pub fn remember(cwd: &std::path::Path, agent: &str, text: &str) -> Result<std::path::PathBuf> {
    let text = text.trim();
    if text.is_empty() {
        anyhow::bail!("nothing to remember");
    }
    let dir = cwd.join(".qoral");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("KNOWLEDGE.md");
    let mut doc = std::fs::read_to_string(&file).unwrap_or_default();
    if doc.trim().is_empty() {
        doc = SKELETON.to_string();
    }
    let today = chrono_date();
    let entry = format!("- ({agent}, {today}) {}", text.split_whitespace().collect::<Vec<_>>().join(" "));
    if !doc.contains("## Notes from agents") {
        doc = format!("{}\n\n## Notes from agents\n", doc.trim_end());
    }
    doc = format!("{}\n{entry}\n", doc.trim_end());
    std::fs::write(&file, doc)?;
    Ok(file)
}

fn chrono_date() -> String {
    // YYYY-MM-DD from unix time, no chrono dependency needed
    let secs = db::now_ms() / 1000;
    let days = secs / 86400;
    // civil-from-days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn list_notes(cwd: &std::path::Path, limit: usize) -> Vec<(String, String)> {
    let dir = cwd.join(".qoral/notes");
    let Ok(rd) = std::fs::read_dir(&dir) else { return vec![] };
    let mut files: Vec<_> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().map(|e| e == "md").unwrap_or(false)).collect();
    files.sort();
    files.reverse();
    files
        .into_iter()
        .take(limit)
        .map(|p| {
            let title = std::fs::read_to_string(&p)
                .ok()
                .and_then(|t| t.lines().find(|l| l.starts_with("# ")).map(|l| l[2..].trim().to_string()))
                .unwrap_or_else(|| p.file_name().unwrap().to_string_lossy().to_string());
            (format!(".qoral/notes/{}", p.file_name().unwrap().to_string_lossy()), title)
        })
        .collect()
}

fn call_tool(db: &Db, me: &str, name: &str, args: &Value) -> Result<Value> {
    let cwd_of_me = || -> std::path::PathBuf {
        db.get_agent(me).ok().flatten().map(|a| std::path::PathBuf::from(a.cwd)).unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    };
    Ok(match name {
        "qoral_whoami" => match db.get_agent(me)? {
            Some(a) => text(format!("You are \"{}\" ({}) in {}. Status: {}.", a.name, a.harness, a.cwd, a.status)),
            None => text(format!("You are \"{me}\".")),
        },
        "qoral_list_agents" => {
            let rows = db.list_agents(false)?;
            if rows.is_empty() {
                return Ok(text("No agents registered."));
            }
            let mut lines: Vec<String> = rows
                .iter()
                .map(|a| {
                    let you = if a.name == me { " (you)" } else { "" };
                    let task = a.task.as_deref().map(|t| format!(" — task: {t}")).unwrap_or_default();
                    format!("- {}{you} [{}] {} in {}{task}", a.name, a.harness, a.status, a.cwd)
                })
                .collect();
            lines.push("- human [operator] reachable via qoral_send(to=\"human\")".into());
            text(lines.join("\n"))
        }
        "qoral_send" => {
            let to = args["to"].as_str().unwrap_or("");
            let message = args["message"].as_str().unwrap_or("");
            match db::send_message(db, me, to, message) {
                Ok(targets) if targets.is_empty() => text("No other agents are running; nobody received the broadcast."),
                Ok(targets) => text(format!("Queued for {}. Use qoral_wait if you expect a reply.", targets.join(", "))),
                Err(e) => error_result(format!("qoral error: {e}")),
            }
        }
        "qoral_inbox" => {
            let msgs = db.unread_for(me)?;
            db.mark_read(&msgs.iter().map(|m| m.id).collect::<Vec<_>>())?;
            if msgs.is_empty() {
                text("Inbox empty.")
            } else {
                text(msgs.iter().map(fmt_msg).collect::<Vec<_>>().join("\n"))
            }
        }
        "qoral_wait" => {
            let timeout = args["timeout_seconds"].as_f64().unwrap_or(120.0).clamp(1.0, 600.0);
            let from = args["from"].as_str().map(|s| s.to_lowercase());
            let deadline = Instant::now() + Duration::from_secs_f64(timeout);
            loop {
                let mut msgs = db.unread_for(me)?;
                if let Some(f) = &from {
                    msgs.retain(|m| &m.sender == f);
                }
                if !msgs.is_empty() {
                    db.mark_read(&msgs.iter().map(|m| m.id).collect::<Vec<_>>())?;
                    return Ok(text(msgs.iter().map(fmt_msg).collect::<Vec<_>>().join("\n")));
                }
                if Instant::now() >= deadline {
                    return Ok(text(format!("No message arrived within {timeout}s. Call qoral_wait again or continue with other work.")));
                }
                std::thread::sleep(Duration::from_millis(400));
            }
        }
        "qoral_spawn" => {
            let max = crate::config::load().max_agents.max(1);
            let running = db.list_agents(true)?.iter().filter(|a| a.harness != "moderator").count();
            if running >= max {
                return Ok(error_result(format!("qoral: {running} agents are already running, which is the max_agents limit ({max}). Reuse an existing agent via qoral_send, or ask the human to raise max_agents in {}/config.json.", paths::home().display())));
            }
            let harness = args["harness"].as_str().unwrap_or("");
            let prompt = args["prompt"].as_str().unwrap_or("");
            let cwd = args["cwd"].as_str().map(|s| s.to_string()).unwrap_or_else(|| cwd_of_me().display().to_string());
            let full = format!("{prompt}\n\n(You were started by agent \"{me}\". Report back to them with qoral_send(to=\"{me}\") when done or if you have questions.)");
            let exe = std::env::current_exe()?;
            let mut cmd = std::process::Command::new(exe);
            cmd.arg("spawn").arg(harness).arg("--dir").arg(&cwd).arg("--no-focus");
            if let Some(n) = args["name"].as_str() {
                cmd.arg("--name").arg(n);
            }
            cmd.arg("--").arg(&full);
            let out = cmd.output()?;
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                let spawned = s.lines().find_map(|l| l.strip_prefix("started ")).map(|l| l.split(' ').next().unwrap_or("").to_string()).unwrap_or_default();
                text(format!("Started agent \"{spawned}\" ({harness}) in {cwd}. It will report back to you; call qoral_wait(from=\"{spawned}\") to receive it."))
            } else {
                error_result(format!("qoral error: {}", String::from_utf8_lossy(&out.stderr).trim()))
            }
        }
        "qoral_log" => {
            let limit = args["limit"].as_u64().unwrap_or(30).min(200) as usize;
            let msgs = db.recent_messages(limit)?;
            if msgs.is_empty() {
                text("No messages yet.")
            } else {
                text(msgs.iter().map(fmt_msg).collect::<Vec<_>>().join("\n"))
            }
        }
        "qoral_remember" => {
            let cwd = cwd_of_me();
            match remember(&cwd, me, args["text"].as_str().unwrap_or("")) {
                Ok(f) => text(format!("Saved to {}. Future agents in this project will see it.", f.display())),
                Err(e) => error_result(format!("qoral error: {e}")),
            }
        }
        "qoral_knowledge" => {
            let cwd = cwd_of_me();
            let know = std::fs::read_to_string(cwd.join(".qoral/KNOWLEDGE.md")).unwrap_or_default();
            let mut out = vec![if know.trim().is_empty() { "(no knowledge recorded yet for this project)".to_string() } else { know.trim().to_string() }];
            let notes = list_notes(&cwd, 15);
            if !notes.is_empty() {
                out.push(String::new());
                out.push("Session notes:".into());
                for (f, t) in notes {
                    out.push(format!("- {f} — {t}"));
                }
            }
            text(out.join("\n"))
        }
        other => anyhow::bail!("unknown tool {other}"),
    })
}

pub fn serve(agent: &str) -> Result<()> {
    let db = Db::open()?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut write = |v: Value| {
        let _ = writeln!(stdout, "{v}");
        let _ = stdout.flush();
    };
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                write(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "parse error" } }));
                continue;
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        if id.is_null() {
            continue; // notification
        }
        let method = msg["method"].as_str().unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result: std::result::Result<Value, (i64, String)> = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params.get("protocolVersion").cloned().unwrap_or(json!("2025-06-18")),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "qoral", "version": env!("CARGO_PKG_VERSION") },
                "instructions": format!("You are agent \"{agent}\" in a qoral workspace. Use qoral_send / qoral_wait to collaborate with the other agents.")
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "resources/list" => Ok(json!({ "resources": [] })),
            "prompts/list" => Ok(json!({ "prompts": [] })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match call_tool(&db, agent, name, &args) {
                    Ok(v) => Ok(v),
                    Err(e) if e.to_string().starts_with("unknown tool") => Err((-32602, e.to_string())),
                    Err(e) => Ok(error_result(format!("qoral error: {e}"))),
                }
            }
            other => Err((-32601, format!("method not found: {other}"))),
        };
        match result {
            Ok(r) => write(json!({ "jsonrpc": "2.0", "id": id, "result": r })),
            Err((code, message)) => write(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })),
        }
    }
    Ok(())
}
