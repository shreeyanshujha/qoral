//! Living documentation: session notes + rolling KNOWLEDGE.md per project, written by a headless model.
//! Also: transcript collection from the harnesses' own session files.
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config;
use crate::db;

const TRANSCRIPT_HEAD: usize = 8000;
const TRANSCRIPT_TAIL: usize = 110_000;
pub const MIN_TRANSCRIPT: usize = 1500;
const NOTE_MARK: &str = "===NOTE===";
const KNOW_MARK: &str = "===KNOWLEDGE===";
const WRITER_SYSTEM: &str = "You are a meticulous technical writer producing internal documentation for a software project from a terminal transcript. You have no tools and cannot read or write files; work only from the text you are given. Follow the formatting instructions exactly and output only the requested documents, with no preamble or commentary.";
const SUMMARIZER_ORDER: &[&str] = &["claude", "codex", "gemini", "agy"];

pub fn knowledge_dir(cwd: &Path) -> PathBuf {
    cwd.join(".qoral")
}
pub fn knowledge_file(cwd: &Path) -> PathBuf {
    knowledge_dir(cwd).join("KNOWLEDGE.md")
}
pub fn notes_dir(cwd: &Path) -> PathBuf {
    knowledge_dir(cwd).join("notes")
}
pub fn read_knowledge(cwd: &Path) -> String {
    std::fs::read_to_string(knowledge_file(cwd)).unwrap_or_default()
}

pub fn which(cmd: &str) -> bool {
    Command::new("sh").arg("-c").arg(format!("command -v {cmd}")).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

pub fn pick_summarizer() -> Option<String> {
    let want = std::env::var("QORAL_SUMMARIZER").ok().unwrap_or_else(|| config::load().summarizer);
    if want == "none" {
        return None;
    }
    if want != "auto" {
        return if crate::paths::HARNESSES.contains(&want.as_str()) && which(&want) { Some(want) } else { None };
    }
    SUMMARIZER_ORDER.iter().find(|h| which(h)).map(|s| s.to_string())
}

pub fn list_notes(cwd: &Path, limit: usize) -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(notes_dir(cwd)) else { return vec![] };
    let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().map(|e| e == "md").unwrap_or(false)).collect();
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

// ---- transcript cleaning --------------------------------------------------

fn is_noise(line: &str) -> bool {
    !line.is_empty() && line.chars().all(|c| c.is_whitespace() || "─━│┃╭╮╰╯├┤┬┴┼═║╔╗╚╝╠╣╦╩╬▀▄█▌▐░▒▓◐◓◑◒⏵·•✻✶✳✢*".contains(c))
}

pub fn strip_ansi(s: &str) -> String {
    let re = regex::Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b[()][A-Za-z0-9]|\x1b[=>]").unwrap();
    re.replace_all(s, "").to_string()
}

pub fn clean_transcript(raw: &str) -> String {
    let stripped = strip_ansi(raw);
    let mut out: Vec<&str> = Vec::new();
    for l in stripped.lines() {
        let l = l.trim_end();
        if is_noise(l) {
            continue;
        }
        if let Some(last) = out.last() {
            if *last == l || (l.is_empty() && last.is_empty()) {
                continue;
            }
        }
        out.push(l);
    }
    let text = out.join("\n").trim().to_string();
    if text.chars().count() > TRANSCRIPT_HEAD + TRANSCRIPT_TAIL {
        let head: String = text.chars().take(TRANSCRIPT_HEAD).collect();
        let tail: String = text.chars().rev().take(TRANSCRIPT_TAIL).collect::<Vec<_>>().into_iter().rev().collect();
        return format!("{head}\n\n[… middle of transcript truncated …]\n\n{tail}");
    }
    text
}

// ---- transcript sources ---------------------------------------------------

pub struct Transcript {
    pub source: String,
    pub text: String,
}

fn clip(s: &str, n: usize) -> String {
    let c = s.chars().count();
    if c > n {
        format!("{} …[+{} chars]", s.chars().take(n).collect::<String>(), c - n)
    } else {
        s.to_string()
    }
}

fn render_content(v: &serde_json::Value, role: &str, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => {
            if !s.trim().is_empty() {
                out.push(format!("{role}: {}", s.trim()));
            }
        }
        serde_json::Value::Array(parts) => {
            for p in parts {
                match p["type"].as_str() {
                    Some("text") => {
                        if let Some(t) = p["text"].as_str() {
                            if !t.trim().is_empty() {
                                out.push(format!("{role}: {}", t.trim()));
                            }
                        }
                    }
                    Some("tool_use") => out.push(format!("TOOL CALL {}({})", p["name"].as_str().unwrap_or("?"), clip(&p["input"].to_string(), 600))),
                    Some("tool_result") => {
                        let body = match &p["content"] {
                            serde_json::Value::Array(a) => a.iter().map(|c| c["text"].as_str().unwrap_or("[non-text]").to_string()).collect::<Vec<_>>().join("\n"),
                            serde_json::Value::String(s) => s.clone(),
                            _ => String::new(),
                        };
                        out.push(format!("TOOL RESULT: {}", clip(&body, 800)));
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

pub fn claude_transcript(session_id: Option<&str>) -> Option<Transcript> {
    let root = dirs::home_dir()?.join(".claude/projects");
    let sid = session_id?;
    let mut file = None;
    for d in std::fs::read_dir(&root).ok()?.flatten() {
        let f = d.path().join(format!("{sid}.jsonl"));
        if f.exists() {
            file = Some(f);
            break;
        }
    }
    let file = file?;
    let mut lines = Vec::new();
    for line in std::fs::read_to_string(&file).ok()?.lines() {
        let Ok(rec) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if rec["isMeta"].as_bool().unwrap_or(false) {
            continue;
        }
        match rec["type"].as_str() {
            Some("user") => render_content(&rec["message"]["content"], "USER", &mut lines),
            Some("assistant") => render_content(&rec["message"]["content"], "ASSISTANT", &mut lines),
            _ => {}
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(Transcript { source: format!("claude transcript {}", file.file_name()?.to_string_lossy()), text: lines.join("\n") })
}

fn walk(dir: &Path, depth: usize, acc: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 {
                walk(&p, depth - 1, acc);
            }
        } else {
            acc.push(p);
        }
    }
}

fn mtime_ms(p: &Path) -> i64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn codex_transcript(cwd: &Path, started_at: i64) -> Option<Transcript> {
    let root = dirs::home_dir()?.join(".codex/sessions");
    let mut files = Vec::new();
    walk(&root, 4, &mut files);
    let mut files: Vec<PathBuf> = files.into_iter().filter(|f| f.extension().map(|e| e == "jsonl").unwrap_or(false) && mtime_ms(f) >= started_at - 60_000).collect();
    files.sort_by_key(|f| std::cmp::Reverse(mtime_ms(f)));
    let cwd_json = serde_json::to_string(&cwd.display().to_string()).ok()?;
    let file = files.into_iter().find(|f| std::fs::read_to_string(f).map(|s| s.chars().take(4000).collect::<String>().contains(&cwd_json)).unwrap_or(false))?;
    let mut lines = Vec::new();
    for line in std::fs::read_to_string(&file).ok()?.lines() {
        let Ok(rec) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let p = if rec["payload"].is_object() { &rec["payload"] } else { &rec };
        let t = p["type"].as_str().or(rec["type"].as_str()).unwrap_or("");
        match t {
            "message" => {
                let role = p["role"].as_str().unwrap_or("?").to_uppercase();
                let text = match &p["content"] {
                    serde_json::Value::Array(a) => a.iter().map(|c| c["text"].as_str().or(c.as_str()).unwrap_or("")).collect::<Vec<_>>().join("\n"),
                    serde_json::Value::String(s) => s.clone(),
                    _ => String::new(),
                };
                if !text.trim().is_empty() {
                    lines.push(format!("{role}: {}", text.trim()));
                }
            }
            "function_call" | "local_shell_call" | "custom_tool_call" => lines.push(format!("TOOL CALL {}({})", p["name"].as_str().unwrap_or(t), clip(&p["arguments"].to_string(), 600))),
            "function_call_output" | "custom_tool_call_output" => lines.push(format!("TOOL RESULT: {}", clip(p["output"].as_str().unwrap_or(""), 800))),
            "agent_message" => lines.push(format!("ASSISTANT: {}", p["message"].as_str().unwrap_or(""))),
            "user_message" => lines.push(format!("USER: {}", p["message"].as_str().unwrap_or(""))),
            _ => {}
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(Transcript { source: format!("codex rollout {}", file.file_name()?.to_string_lossy()), text: lines.join("\n") })
}

pub fn gemini_transcript(started_at: i64) -> Option<Transcript> {
    let root = dirs::home_dir()?.join(".gemini/tmp");
    let mut files = Vec::new();
    walk(&root, 3, &mut files);
    let mut files: Vec<PathBuf> = files
        .into_iter()
        .filter(|f| {
            let s = f.display().to_string();
            (s.contains("/chats/") || f.file_name().map(|n| n.to_string_lossy().starts_with("session-")).unwrap_or(false)) && s.ends_with(".json") && mtime_ms(f) >= started_at - 60_000
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(mtime_ms(f)));
    let file = files.into_iter().next()?;
    let data: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).ok()?).ok()?;
    let msgs = data["messages"].as_array().or(data["history"].as_array())?;
    let mut lines = Vec::new();
    for m in msgs {
        let role = m["type"].as_str().or(m["role"].as_str()).unwrap_or("MESSAGE").to_uppercase().replace("GEMINI", "ASSISTANT").replace("MODEL", "ASSISTANT");
        let content = match &m["content"] {
            serde_json::Value::Array(a) => a.iter().map(|c| c["text"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n"),
            serde_json::Value::String(s) => s.clone(),
            _ => m["text"].as_str().unwrap_or("").to_string(),
        };
        if !content.trim().is_empty() {
            lines.push(format!("{role}: {}", content.trim()));
        }
        if let Some(tcs) = m["toolCalls"].as_array() {
            for tc in tcs {
                lines.push(format!("TOOL CALL {}({})", tc["name"].as_str().unwrap_or("?"), clip(&tc["args"].to_string(), 600)));
                if let Some(r) = tc.get("result") {
                    lines.push(format!("TOOL RESULT: {}", clip(&r.to_string(), 800)));
                }
            }
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(Transcript { source: format!("gemini chat {}", file.file_name()?.to_string_lossy()), text: lines.join("\n") })
}

/// Antigravity keeps protobuf blobs in SQLite; pull the readable runs out of each step payload.
pub fn agy_transcript(cwd: &Path, started_at: i64) -> Option<Transcript> {
    let dir = dirs::home_dir()?.join(".gemini/antigravity-cli/conversations");
    let mut dbs: Vec<PathBuf> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "db").unwrap_or(false) && mtime_ms(p) >= started_at - 60_000).collect();
    dbs.sort_by_key(|f| std::cmp::Reverse(mtime_ms(f)));
    let uuid_re = regex::Regex::new(r"(?i)[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}").ok()?;
    let word_re = regex::Regex::new(r"[A-Za-z]{2,}").ok()?;
    let cwd_s = cwd.display().to_string();
    let mut best: Option<(bool, Transcript)> = None;
    for f in dbs {
        let Ok(conn) = rusqlite::Connection::open_with_flags(&f, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else { continue };
        let meta: Option<Vec<u8>> = conn.query_row("SELECT data FROM trajectory_metadata_blob LIMIT 1", [], |r| r.get(0)).ok();
        let matches_cwd = meta.map(|m| String::from_utf8_lossy(&m).contains(&cwd_s)).unwrap_or(false);
        let Ok(mut st) = conn.prepare("SELECT step_payload FROM steps ORDER BY idx ASC") else { continue };
        let mut seen = std::collections::HashSet::new();
        let mut lines = Vec::new();
        let rows = st.query_map([], |r| r.get::<_, Option<Vec<u8>>>(0));
        if let Ok(rows) = rows {
            for blob in rows.flatten().flatten() {
                let s = String::from_utf8_lossy(&blob);
                let mut run = String::new();
                let mut flush = |run: &mut String, lines: &mut Vec<String>| {
                    let r = uuid_re.replace_all(run, "").to_string();
                    let r = r.trim_start_matches(|c: char| !(c.is_ascii_alphabetic() || "{[\"/".contains(c))).trim().to_string();
                    if r.chars().count() >= 24 && word_re.find_iter(&r).count() >= 4 && seen.insert(r.clone()) {
                        lines.push(r);
                    }
                    run.clear();
                };
                for ch in s.chars() {
                    if ch == '\n' || ch == '\t' || (ch >= ' ' && ch != '\u{fffd}' && !ch.is_control()) {
                        run.push(ch);
                    } else {
                        flush(&mut run, &mut lines);
                    }
                }
                flush(&mut run, &mut lines);
            }
        }
        if lines.is_empty() {
            continue;
        }
        let t = Transcript { source: format!("agy conversation {}", f.file_name()?.to_string_lossy()), text: lines.join("\n") };
        match &best {
            Some((true, _)) => {}
            Some((false, _)) if !matches_cwd => {}
            _ => best = Some((matches_cwd, t)),
        }
    }
    best.map(|(_, t)| t)
}

pub fn raw_log_transcript(path: &Path) -> Option<Transcript> {
    let data = std::fs::read(path).ok()?;
    if data.is_empty() {
        return None;
    }
    let start = data.len().saturating_sub(4 * 1024 * 1024);
    let s = String::from_utf8_lossy(&data[start..]);
    let re = regex::Regex::new(r"\x1b\[[0-9;]*[ABCDEFGHJKSTf]").ok()?;
    let text = re.replace_all(&s, "\n").replace('\r', "\n");
    Some(Transcript { source: "raw pane log".into(), text })
}

pub struct SessionInfo<'a> {
    pub name: &'a str,
    pub harness: &'a str,
    pub cwd: &'a Path,
    pub task: Option<&'a str>,
    pub started_at: i64,
    pub ended_at: i64,
    pub session_id: Option<&'a str>,
    pub raw_log: Option<&'a Path>,
    /// The emulator's screen + scrollback, if the caller has it.
    pub scrollback: Option<&'a str>,
}

/// Richest transcript available, preferring harness session files.
pub fn collect_transcript(s: &SessionInfo) -> Option<Transcript> {
    let mut attempts: Vec<Transcript> = Vec::new();
    let mut push = |t: Option<Transcript>| {
        if let Some(t) = t {
            attempts.push(t);
        }
    };
    match s.harness {
        "claude" => push(claude_transcript(s.session_id)),
        "codex" => push(codex_transcript(s.cwd, s.started_at)),
        "gemini" => push(gemini_transcript(s.started_at)),
        "agy" => push(agy_transcript(s.cwd, s.started_at)),
        _ => {}
    }
    if let Some(p) = s.raw_log {
        push(raw_log_transcript(p));
    }
    if let Some(sb) = s.scrollback {
        if !sb.trim().is_empty() {
            push(Some(Transcript { source: "scrollback".into(), text: sb.to_string() }));
        }
    }
    if let Some(i) = attempts.iter().position(|a| clean_transcript(&a.text).chars().count() >= MIN_TRANSCRIPT) {
        return Some(attempts.swap_remove(i));
    }
    attempts.into_iter().max_by_key(|a| a.text.len())
}

// ---- summarizer -----------------------------------------------------------

pub fn run_summarizer(prompt: &str, input: &str, cwd: &Path, summarizer: &str) -> Result<String> {
    let model = std::env::var("QORAL_SUMMARIZER_MODEL").ok().or_else(|| config::load().model);
    let (mut cmd, stdin_text): (Command, String) = match summarizer {
        "claude" => {
            let mut c = Command::new("claude");
            c.args(["-p", "--output-format", "text", "--tools", "", "--strict-mcp-config", "--system-prompt", WRITER_SYSTEM]);
            if let Some(m) = &model {
                c.args(["--model", m]);
            }
            c.arg(prompt);
            (c, input.to_string())
        }
        "codex" => {
            let mut c = Command::new("codex");
            c.args(["exec", "--skip-git-repo-check", "-C"]).arg(cwd).args(["-s", "read-only"]);
            if let Some(m) = &model {
                c.args(["-m", m]);
            }
            (c, format!("{prompt}\n\n{input}"))
        }
        "gemini" => {
            let mut c = Command::new("gemini");
            c.args(["-p", prompt]);
            if let Some(m) = &model {
                c.args(["-m", m]);
            }
            (c, input.to_string())
        }
        "agy" => {
            let mut c = Command::new("agy");
            c.args(["-p", &format!("{prompt}\n\n{input}"), "--disable-slash-commands"]);
            if let Some(m) = &model {
                c.args(["--model", m]);
            }
            (c, String::new())
        }
        other => bail!("unknown summarizer {other}"),
    };
    cmd.current_dir(cwd).env_remove("QORAL_AGENT").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().with_context(|| format!("run {summarizer}"))?;
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(stdin_text.as_bytes());
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("{summarizer} exited {}: {}", out.status, err.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect::<String>().trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn unfence(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_prefix("```markdown").or_else(|| t.strip_prefix("```md")).or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim().to_string()
}

fn split_output(output: &str) -> Option<(String, String, bool)> {
    let i = output.find(NOTE_MARK);
    let j = output.find(KNOW_MARK);
    match (i, j) {
        (Some(i), Some(j)) if j > i => Some((unfence(&output[i + NOTE_MARK.len()..j]), unfence(&output[j + KNOW_MARK.len()..]), false)),
        (Some(i), _) => Some((unfence(&output[i + NOTE_MARK.len()..]), String::new(), false)),
        _ => {
            let t = output.trim();
            if t.is_empty() { None } else { Some((unfence(t), String::new(), true)) }
        }
    }
}

pub fn stamp_now() -> String {
    chrono::Local::now().format("%Y-%m-%d-%H%M").to_string()
}

fn build_prompt(s: &SessionInfo) -> String {
    let when = chrono::DateTime::from_timestamp_millis(s.started_at).map(|d| d.with_timezone(&chrono::Local).to_rfc2822()).unwrap_or_else(|| "unknown".into());
    let mins = (s.ended_at - s.started_at) / 60_000;
    format!(
        r#"You are writing documentation for a coding-agent session that just ended inside the "qoral" multi-agent workspace.

Project directory: {cwd}
Agent: {name} ({harness}), started {when}, ran {mins} min
Task given at launch: {task}

After this prompt you will receive the CURRENT KNOWLEDGE document for the project (may be empty) and then the TERMINAL TRANSCRIPT of the session (it may contain rendering noise, tool output and other agents' messages prefixed "[qoral]").

Produce exactly two markdown documents, separated by the marker lines shown, and nothing else.

{note}
A session note. Use these sections; omit a section only if it would be empty:
# <one-line title: what the session accomplished>
## Summary
2–5 sentences.
## Changes
Files created / modified / deleted and what changed, as bullets with paths verbatim.
## Decisions
Choices made and the reasoning.
## Learnings
Gotchas, non-obvious facts about the codebase, commands that worked or failed.
## Open threads
Unfinished work, TODOs, questions for the next agent.
{know}
The updated project knowledge document. Start from CURRENT KNOWLEDGE, merge in durable facts from this session (architecture, layout, conventions, decisions, gotchas, how to run and test), drop anything the session showed to be outdated, deduplicate, and keep the whole document under 6000 characters. It is injected into every future agent's system prompt for this project, so keep only what a new agent needs; no session-specific chatter. Use the headings: # Project knowledge, ## Overview, ## Layout, ## Conventions, ## Decisions, ## Gotchas, ## How to run & test, ## Notes from agents. Preserve the existing entries under "Notes from agents" (agents added them live) unless they are clearly obsolete."#,
        cwd = s.cwd.display(),
        name = s.name,
        harness = s.harness,
        task = s.task.unwrap_or("(none; interactive session)"),
        note = NOTE_MARK,
        know = KNOW_MARK,
    )
}

pub enum DocOutcome {
    Skipped(String),
    Written {
        note: PathBuf,
        knowledge: Option<PathBuf>,
        summarizer: String,
        #[allow(dead_code)]
        source: String,
    },
}

/// Summarize a session into .qoral/notes and merge .qoral/KNOWLEDGE.md.
pub fn document_session(s: &SessionInfo, log: &mut dyn FnMut(&str)) -> Result<DocOutcome> {
    if !config::load().document_sessions {
        return Ok(DocOutcome::Skipped("document_sessions=false in config".into()));
    }
    let Some(summarizer) = pick_summarizer() else { return Ok(DocOutcome::Skipped("no summarizer CLI available".into())) };
    let Some(t) = collect_transcript(s) else { return Ok(DocOutcome::Skipped("no transcript found".into())) };
    let text = clean_transcript(&t.text);
    if text.chars().count() < MIN_TRANSCRIPT {
        return Ok(DocOutcome::Skipped("session too short to document".into()));
    }
    log(&format!("transcript source: {}", t.source));
    log(&format!("summarizing {}k chars of transcript with {summarizer}…", text.chars().count() / 1000));
    let current = read_knowledge(s.cwd);
    let input = format!("CURRENT KNOWLEDGE:\n{}\n\nTERMINAL TRANSCRIPT:\n{text}", if current.trim().is_empty() { "(empty)" } else { current.trim() });
    let output = run_summarizer(&build_prompt(s), &input, s.cwd, &summarizer)?;
    let Some((note, knowledge, degraded)) = split_output(&output) else {
        std::fs::create_dir_all(notes_dir(s.cwd))?;
        let raw = notes_dir(s.cwd).join(format!("{}-{}.raw.txt", stamp_now(), s.name));
        std::fs::write(&raw, &output)?;
        bail!("summarizer returned nothing usable; raw output kept at {}", raw.display());
    };
    if degraded {
        log("summarizer ignored the format; saved its output as the note, knowledge left unchanged");
    }
    std::fs::create_dir_all(notes_dir(s.cwd))?;
    let note_path = notes_dir(s.cwd).join(format!("{}-{}.md", stamp_now(), s.name));
    let header = format!(
        "---\nagent: {}\nharness: {}\ntask: {}\nstarted: {}\nended: {}\nsource: {}\n---\n\n",
        s.name,
        s.harness,
        serde_json::to_string(s.task.unwrap_or("")).unwrap_or_default(),
        chrono::DateTime::from_timestamp_millis(s.started_at).map(|d| d.to_rfc3339()).unwrap_or_default(),
        chrono::DateTime::from_timestamp_millis(s.ended_at).map(|d| d.to_rfc3339()).unwrap_or_default(),
        t.source,
    );
    std::fs::write(&note_path, format!("{header}{note}\n"))?;
    let mut know_path = None;
    if knowledge.len() > 40 {
        let kp = knowledge_file(s.cwd);
        if !current.is_empty() {
            let _ = std::fs::write(knowledge_dir(s.cwd).join("KNOWLEDGE.prev.md"), &current);
        }
        std::fs::write(&kp, format!("{knowledge}\n"))?;
        know_path = Some(kp);
    }
    Ok(DocOutcome::Written { note: note_path, knowledge: know_path, summarizer, source: t.source })
}

/// Insert a line under "## Decisions" in KNOWLEDGE.md (creating structure as needed).
pub fn append_decision(cwd: &Path, line: &str) -> Result<()> {
    std::fs::create_dir_all(knowledge_dir(cwd))?;
    let mut doc = read_knowledge(cwd);
    if doc.trim().is_empty() {
        doc = "# Project knowledge\n\n## Decisions\n".into();
    }
    if !doc.lines().any(|l| l.trim() == "## Decisions") {
        doc = format!("{}\n\n## Decisions\n", doc.trim_end());
    }
    let mut out = String::new();
    let mut inserted = false;
    let mut in_dec = false;
    for l in doc.lines() {
        if in_dec && l.starts_with("## ") && !inserted {
            out.push_str(line);
            out.push_str("\n\n");
            inserted = true;
            in_dec = false;
        }
        out.push_str(l);
        out.push('\n');
        if l.trim() == "## Decisions" {
            in_dec = true;
        }
    }
    if !inserted {
        out = format!("{}\n{line}\n", out.trim_end());
    }
    std::fs::write(knowledge_file(cwd), out)?;
    Ok(())
}

#[allow(dead_code)]
pub fn _keep(_: db::Message) {}
