//! `qoral debate`: several agents argue a question under a deterministic moderator (this process),
//! then a headless model writes the decision into <project>/.qoral/decisions/ and links it from KNOWLEDGE.md.
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::db::{self, Db};
use crate::knowledge;
use crate::proto::{ClientMsg, DaemonMsg, SpawnReq};
use crate::{ctl, paths};

pub const MODERATOR: &str = "moderator";

struct Persona {
    name: &'static str,
    brief: &'static str,
}

const PERSONAS: &[Persona] = &[
    Persona { name: "pragmatist", brief: "You care about what ships and what the team can maintain: follow the project's existing conventions unless there is a strong reason not to, prefer boring proven approaches, and weigh implementation effort honestly." },
    Persona { name: "skeptic", brief: "You care about correctness, edge cases and failure modes. Attack every proposal for what breaks, what is untested, and what surprises callers. Hold your position for at least two rounds unless someone shows a concrete flaw in it." },
    Persona { name: "minimalist", brief: "You argue for the simplest thing that can work: fewest concepts, least code, no speculative generality. Push back on anything that adds a dependency, an abstraction or a configuration knob without a present need." },
    Persona { name: "architect", brief: "You take the long view: how the choice constrains future changes, performance at scale, API ergonomics for callers, and consistency with the wider codebase." },
];

pub struct DebateOpts {
    pub question: String,
    pub cwd: PathBuf,
    pub harnesses: Vec<String>,
    pub rounds: usize,
    pub timeout: Duration,
    pub build: bool,
    pub keep: bool,
}

pub fn default_debaters(count: usize) -> Result<Vec<String>> {
    let avail: Vec<&str> = ["claude", "agy", "codex", "gemini"].into_iter().filter(|h| knowledge::which(h)).collect();
    if avail.is_empty() {
        bail!("no agent CLIs found on PATH");
    }
    Ok((0..count).map(|i| avail[i % avail.len()].to_string()).collect())
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.len() >= 48 {
            break;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "debate".into() } else { out }
}

struct Reply {
    body: String,
    stance: Option<String>,
    consensus: bool,
}

fn parse_reply(body: &str) -> Reply {
    let stance_re = regex::Regex::new(r"(?i)STANCE:\s*(.+)").unwrap();
    let cons_re = regex::Regex::new(r"(?i)CONSENSUS:\s*(yes|no)").unwrap();
    let stance = stance_re.captures_iter(body).last().map(|c| c[1].trim().to_string());
    let consensus = cons_re.captures_iter(body).last().map(|c| c[1].to_lowercase() == "yes").unwrap_or(false);
    Reply { body: body.to_string(), stance, consensus }
}

fn debater_brief(name: &str, persona: &str, question: &str, cwd: &Path, participants: &[String], rounds: usize) -> String {
    let others: Vec<&String> = participants.iter().filter(|p| p.as_str() != name).collect();
    let others_s = others.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" and ");
    format!(
        "You are \"{name}\", one of {n} participants ({all}) in a structured debate run by \"{mod_}\". The moderator is an automated process on the qoral bus, not a person: it only reads messages sent to it and relays positions between participants.\n\n\
QUESTION: {question}\n\n\
YOUR PERSPECTIVE: {persona}\n\n\
Ground every claim in this project ({cwd}): read the relevant files first with your file-reading tools (avoid shell commands; they may block on a permission prompt nobody is watching). Do NOT edit, create or delete any files during the debate.\n\n\
PROTOCOL ({rounds} rounds max):\n\
1. Investigate briefly, then send your opening position with qoral_send(to=\"{mod_}\"): at most 250 words, concrete, citing files where relevant. End the message with exactly these two lines:\n\
   STANCE: <one-line summary of the approach you advocate>\n\
   CONSENSUS: no\n\
2. Immediately call qoral_wait(timeout_seconds=600). The moderator will send you the other participants' positions.\n\
3. Each round: engage with the strongest points from {others_s}, say explicitly what changed your mind (if anything), and send your updated position to {mod_} ending with the same two lines. Write \"CONSENSUS: yes\" only when you fully accept ONE shared approach; then your STANCE line must describe that shared approach in the same words the others would use.\n\
4. After each reply call qoral_wait(timeout_seconds=600) again. Stop when the moderator says the debate is over.\n\n\
Rules: send everything to {mod_}, never to the other participants or to human directly. One message per round. Be direct; concede when the argument against you is better, hold firm when it is not.",
        n = participants.len(),
        all = participants.join(", "),
        mod_ = MODERATOR,
        cwd = cwd.display(),
    )
}

fn round_message(round: usize, rounds: usize, positions: &[(String, Reply)], me: &str) -> String {
    let mut parts = vec![format!("Round {round} of {rounds}. Positions from the other participants:")];
    for (n, r) in positions.iter().filter(|(n, _)| n != me) {
        parts.push(String::new());
        parts.push(format!("--- {n} ---"));
        parts.push(r.body.trim().to_string());
    }
    parts.push(String::new());
    parts.push(format!("Respond now: critique these, update your position if their argument is better, and reply to {MODERATOR} ending with your STANCE and CONSENSUS lines. Then qoral_wait again."));
    parts.join("\n")
}

fn synthesis_prompt(question: &str, cwd: &Path, consensus: bool, participants: &[String], stances: &[(String, Option<String>)]) -> String {
    format!(
        "You are the moderator's scribe for a structured design debate between AI coding agents working in the project at {cwd}.\n\n\
QUESTION: {question}\n\n\
Participants: {parts}.\n\
Outcome: {outcome}\n\
Final stances:\n{stances}\n\n\
After this prompt you will receive the CURRENT PROJECT KNOWLEDGE and the FULL DEBATE TRANSCRIPT.\n\n\
Write one markdown document and nothing else, with exactly these sections:\n\
# Decision: <short title>\n\
**Question:** one line.\n\
**Decision:** the chosen approach in 1–3 sentences (label it \"Recommendation\" instead of \"Decision\" if there was no consensus).\n\
## Rationale\nThe strongest arguments that carried, grounded in the project's files and conventions.\n\
## Alternatives considered\nEach rejected option and the concrete reason it lost.\n\
## Dissent and risks\nRemaining objections, edge cases to watch, and what would change the decision.\n\
## Implementation plan\nNumbered, concrete steps with file paths, tests to add, and how to verify.\n\
## Participants\nOne line per participant: perspective and how their position moved.",
        cwd = cwd.display(),
        parts = participants.join(", "),
        outcome = if consensus { "the participants reached consensus." } else { "the participants did NOT fully converge within the round limit; you must pick the best-supported position and say why." },
        stances = stances.iter().map(|(n, s)| format!("- {n}: {}", s.clone().unwrap_or_else(|| "(no explicit stance)".into()))).collect::<Vec<_>>().join("\n"),
    )
}

async fn spawn(req: SpawnReq) -> Result<String> {
    match ctl::request(ClientMsg::Spawn(req), true).await? {
        DaemonMsg::Spawned { name, .. } => Ok(name),
        DaemonMsg::Error { message } => bail!("{message}"),
        other => bail!("unexpected reply {other:?}"),
    }
}

async fn kill(name: &str) {
    let _ = ctl::request(ClientMsg::Kill { name: name.to_string() }, false).await;
}

pub async fn run(opts: DebateOpts, log: &dyn Fn(&str)) -> Result<()> {
    let question = opts.question.trim().to_string();
    if question.is_empty() {
        bail!("a question is required");
    }
    let cwd = if opts.cwd.is_absolute() { opts.cwd.clone() } else { std::env::current_dir()?.join(&opts.cwd) };
    let harnesses = if opts.harnesses.is_empty() { default_debaters(3)? } else { opts.harnesses.clone() };
    if harnesses.len() < 2 {
        bail!("a debate needs at least two participants");
    }
    if harnesses.len() > PERSONAS.len() {
        bail!("at most {} participants", PERSONAS.len());
    }
    for h in &harnesses {
        if !paths::HARNESSES.contains(&h.as_str()) {
            bail!("unknown harness \"{h}\"");
        }
    }
    let db = Db::open()?;
    let t0 = Instant::now();
    let el = || format!("[{}s]", t0.elapsed().as_secs());

    // moderator: a virtual agent row so participants can address it
    let _ = db.delete_agent(MODERATOR);
    db.insert_agent(MODERATOR, "moderator", &cwd.display().to_string(), None, Some(&format!("debate: {}", question.chars().take(160).collect::<String>())))?;
    db.set_status(MODERATOR, "moderating")?;

    let mut names: Vec<String> = Vec::new();
    for (i, _) in harnesses.iter().enumerate() {
        let mut n = PERSONAS[i].name.to_string();
        if db.get_agent(&n)?.map(|a| a.exited_at.is_none()).unwrap_or(false) {
            n = format!("{n}-{:x}", db::now_ms() % 4096);
        }
        names.push(n);
    }
    let mut spawned: Vec<String> = Vec::new();
    let mut transcript: Vec<(usize, String, String)> = Vec::new();
    let mut last_id = db.recent_messages(1)?.first().map(|m| m.id).unwrap_or(0);

    log(&format!("debate: {question}"));
    log(&format!("project: {}", cwd.display()));
    log(&format!("participants: {} · {} rounds max · {}s per round", names.iter().zip(&harnesses).map(|(n, h)| format!("{n} ({h})")).collect::<Vec<_>>().join(", "), opts.rounds, opts.timeout.as_secs()));

    let result: Result<PathBuf> = async {
        for (i, h) in harnesses.iter().enumerate() {
            let brief = debater_brief(&names[i], PERSONAS[i].brief, &question, &cwd, &names, opts.rounds);
            let n = spawn(SpawnReq { harness: h.clone(), name: Some(names[i].clone()), cwd: Some(cwd.display().to_string()), prompt: Some(brief), focus: false, command: None }).await?;
            spawned.push(n.clone());
            log(&format!("{} spawned {n} ({h})", el()));
        }

        let mut positions: Vec<(String, Reply)> = Vec::new();
        let mut consensus = false;
        let mut rounds_run = 0;
        let mut warned: std::collections::HashSet<String> = std::collections::HashSet::new();

        for round in 1..=opts.rounds {
            rounds_run = round;
            if round > 1 {
                for n in &names {
                    if !positions.iter().any(|(p, _)| p == n) && round > 2 {
                        continue;
                    }
                    db.add_message(MODERATOR, n, &round_message(round, opts.rounds, &positions, n))?;
                }
                log(&format!("{} round {round}: positions relayed, waiting for replies…", el()));
            } else {
                log(&format!("{} round 1: waiting for opening positions…", el()));
            }
            // collect
            let mut got: Vec<(String, Reply)> = Vec::new();
            let deadline = Instant::now() + opts.timeout;
            while got.len() < names.len() && Instant::now() < deadline {
                for m in db.messages_since(last_id, 500)? {
                    last_id = last_id.max(m.id);
                    if m.recipient != MODERATOR || !names.contains(&m.sender) {
                        continue;
                    }
                    db.mark_read(&[m.id])?;
                    let r = parse_reply(&m.body);
                    log(&format!("{} round {round}: {} → {}{}", el(), m.sender, r.stance.as_deref().map(|s| format!("STANCE: {s}")).unwrap_or_else(|| "(no stance line)".into()), if r.consensus { "  [consensus: yes]" } else { "" }));
                    transcript.push((round, m.sender.clone(), m.body.clone()));
                    if let Some(slot) = got.iter_mut().find(|(n, _)| *n == m.sender) {
                        if r.stance.is_some() || slot.1.stance.is_none() {
                            slot.1 = r;
                        }
                    } else {
                        got.push((m.sender.clone(), r));
                    }
                }
                for n in &names {
                    if got.iter().any(|(g, _)| g == n) || warned.contains(&format!("{round}:{n}")) {
                        continue;
                    }
                    if let Some(a) = db.get_agent(n)? {
                        if a.status == "attention" {
                            warned.insert(format!("{round}:{n}"));
                            log(&format!("{} {n} is waiting on a prompt in its window (permission/trust). Answer it in qoral to let it continue.", el()));
                            db.add_message(MODERATOR, "human", &format!("{n} is blocked on a prompt in its window during the debate. Select it in the sidebar and answer it."))?;
                        } else if a.exited_at.is_some() {
                            warned.insert(format!("{round}:{n}"));
                            log(&format!("{} {n} exited; continuing without them", el()));
                        }
                    }
                }
                if got.len() < names.len() {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                }
            }
            let missing: Vec<&String> = names.iter().filter(|n| !got.iter().any(|(g, _)| g == *n)).collect();
            if !missing.is_empty() {
                log(&format!("{} round {round}: no reply from {} within {}s; continuing without them", el(), missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "), opts.timeout.as_secs()));
            }
            for (n, r) in got.iter() {
                if let Some(slot) = positions.iter_mut().find(|(p, _)| p == n) {
                    slot.1 = Reply { body: r.body.clone(), stance: r.stance.clone(), consensus: r.consensus };
                } else {
                    positions.push((n.clone(), Reply { body: r.body.clone(), stance: r.stance.clone(), consensus: r.consensus }));
                }
            }
            if got.len() >= 2 && got.len() == names.len() && got.iter().all(|(_, r)| r.consensus) {
                consensus = true;
                log(&format!("{} consensus reached after {round} round(s)", el()));
                break;
            }
            if got.is_empty() {
                log(&format!("{} nobody replied; ending debate", el()));
                break;
            }
        }
        for n in &names {
            db.add_message(MODERATOR, n, "The debate is over. Thank you. Do not send further messages; stop and wait.")?;
        }

        // synthesis
        let stances: Vec<(String, Option<String>)> = names.iter().map(|n| (n.clone(), positions.iter().find(|(p, _)| p == n).and_then(|(_, r)| r.stance.clone()))).collect();
        let dec_dir = knowledge::knowledge_dir(&cwd).join("decisions");
        std::fs::create_dir_all(&dec_dir)?;
        let path = dec_dir.join(format!("{}-{}.md", knowledge::stamp_now(), slugify(&question)));
        let debate_text = transcript.iter().map(|(r, f, b)| format!("### round {r} — {f}\n{}", b.trim())).collect::<Vec<_>>().join("\n\n");
        let mut body: Option<String> = None;
        if let Some(sum) = knowledge::pick_summarizer() {
            log(&format!("{} writing decision document with {sum}…", el()));
            let input = format!("CURRENT PROJECT KNOWLEDGE:\n{}\n\nFULL DEBATE TRANSCRIPT:\n{debate_text}", { let k = knowledge::read_knowledge(&cwd); if k.trim().is_empty() { "(empty)".to_string() } else { k } });
            match knowledge::run_summarizer(&synthesis_prompt(&question, &cwd, consensus, &names, &stances), &input, &cwd, &sum) {
                Ok(out) => {
                    let t = out.trim().trim_start_matches("```markdown").trim_start_matches("```md").trim_start_matches("```").trim_end_matches("```").trim().to_string();
                    body = Some(t);
                }
                Err(e) => log(&format!("{} synthesis failed ({e}); saving raw transcript instead", el())),
            }
        }
        let body = body.unwrap_or_else(|| {
            format!(
                "# Decision: {question}\n\n**Question:** {question}\n\n**Outcome:** {}\n\n## Final stances\n{}\n",
                if consensus { "consensus" } else { "no consensus" },
                stances.iter().map(|(n, s)| format!("- **{n}**: {}", s.clone().unwrap_or_else(|| "(none)".into()))).collect::<Vec<_>>().join("\n")
            )
        });
        let header = format!(
            "---\nquestion: {}\nparticipants: {}\nharnesses: {}\nrounds: {rounds_run}\nconsensus: {consensus}\ndate: {}\n---\n\n",
            serde_json::to_string(&question)?,
            names.join(", "),
            harnesses.join(", "),
            chrono::Local::now().to_rfc3339()
        );
        std::fs::write(&path, format!("{header}{}\n\n---\n\n## Debate transcript\n\n{debate_text}\n", body.trim()))?;
        let title = body.lines().find_map(|l| l.strip_prefix("# Decision:")).map(|s| s.trim().to_string()).unwrap_or_else(|| question.clone());
        let rel = path.strip_prefix(&cwd).map(|p| p.display().to_string()).unwrap_or_else(|_| path.display().to_string());
        knowledge::append_decision(&cwd, &format!("- ({}, debate{}) {title} — see {rel}", chrono::Local::now().format("%Y-%m-%d"), if consensus { "" } else { ", no consensus" }))?;
        log(&format!("{} decision: {}", el(), path.display()));
        db.add_message(MODERATOR, "human", &format!("DECISION{}: {title}. Full write-up: {rel}", if consensus { "" } else { " (no consensus; recommendation)" }))?;

        if opts.build {
            let bh = if harnesses.iter().any(|h| h == "claude") { "claude".to_string() } else { harnesses[0].clone() };
            let bname = if db.get_agent("builder")?.map(|a| a.exited_at.is_none()).unwrap_or(false) { format!("builder-{:x}", db::now_ms() % 4096) } else { "builder".to_string() };
            let prompt = format!(
                "Implement the decision recorded in {rel} (read it first, including the implementation plan). Follow the project's conventions, add or update tests as the plan says, run the test suite, and when everything passes send the human a short report with qoral_send(to=\"human\") listing the files you changed. If the plan turns out to be infeasible, stop and explain to human instead of improvising a different design."
            );
            let n = spawn(SpawnReq { harness: bh.clone(), name: Some(bname), cwd: Some(cwd.display().to_string()), prompt: Some(prompt), focus: true, command: None }).await?;
            log(&format!("{} builder {n} ({bh}) started", el()));
        }
        log("");
        log(body.trim());
        Ok(path)
    }
    .await;

    if !opts.keep {
        tokio::time::sleep(Duration::from_secs(4)).await;
        for n in &spawned {
            kill(n).await;
        }
        log(&format!("{} participants dismissed (use --keep to leave them running)", el()));
    } else {
        log(&format!("{} participants kept running: {}", el(), spawned.join(", ")));
    }
    let _ = db.delete_agent(MODERATOR);
    result.map(|_| ())
}
