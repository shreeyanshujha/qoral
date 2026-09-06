//! End-to-end test of the daemon with a fake `claude` on PATH (tests/fixtures/claude):
//! up → spawn → idle detection → nudge typed into the PTY → MCP → kill → stop.
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_qoral"))
}

struct Env {
    home: PathBuf,
    fake_log: PathBuf,
    path: String,
    _tmp: tempfile::TempDir,
}

fn setup() -> Env {
    // short path: the daemon socket lives in the runtime dir, but keep the home short anyway
    let tmp = tempfile::Builder::new().prefix("qoral-e2e").tempdir_in(std::env::temp_dir()).unwrap();
    let home = tmp.path().join("home");
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test/fixtures");
    let path = format!("{}:{}", fixtures.display(), std::env::var("PATH").unwrap_or_default());
    Env { home, fake_log: tmp.path().join("fake.log"), path, _tmp: tmp }
}

fn qoral(env: &Env, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .env("QORAL_HOME", &env.home)
        .env("QORAL_SUMMARIZER", "none")
        .env("FAKE_LOG", &env.fake_log)
        .env("PATH", &env.path)
        .output()
        .expect("run qoral")
}

fn out(o: &std::process::Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn wait_for(mut pred: impl FnMut() -> bool, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    pred()
}

#[test]
fn workspace_roundtrip() {
    let env = setup();
    let proj = env.home.join("../proj");
    std::fs::create_dir_all(&proj).unwrap();
    let log = || std::fs::read_to_string(&env.fake_log).unwrap_or_default();

    let up = qoral(&env, &["up"]);
    assert!(up.status.success(), "up: {}", out(&up));

    let sp = qoral(&env, &["spawn", "claude", "--dir", proj.to_str().unwrap(), "--name", "fake", "--no-focus", "do the thing"]);
    assert!(out(&sp).contains("started fake (claude)"), "spawn: {}", out(&sp));

    assert!(wait_for(|| log().contains("--mcp-config") && log().contains("do the thing"), 15), "fake never started: {}", log());
    assert!(wait_for(|| out(&qoral(&env, &["list"])).contains("idle"), 15), "not idle: {}", out(&qoral(&env, &["list"])));

    let send = qoral(&env, &["send", "fake", "hello from the test"]);
    assert!(out(&send).contains("queued for fake"), "{}", out(&send));
    assert!(wait_for(|| log().contains("INPUT: [qoral] message from human: hello from the test"), 20), "nudge not delivered: {}", log());

    // MCP roundtrip
    let mut child = Command::new(bin())
        .args(["mcp", "--agent", "fake"])
        .env("QORAL_HOME", &env.home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"qoral_send","arguments":{"to":"human","message":"done"}}}
"#)
        .unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(String::from_utf8_lossy(&o.stdout).contains("Queued for human"), "{}", String::from_utf8_lossy(&o.stdout));
    assert!(out(&qoral(&env, &["log", "-n", "5"])).contains("fake → human: done"));

    // type a line into the agent
    let t = qoral(&env, &["type", "fake", "typed line"]);
    assert!(t.status.success(), "{}", out(&t));
    assert!(wait_for(|| log().contains("INPUT: typed line"), 10), "type not delivered: {}", log());

    let k = qoral(&env, &["kill", "fake", "--no-docs"]);
    assert!(out(&k).contains("killed fake"), "{}", out(&k));
    assert!(wait_for(|| !out(&qoral(&env, &["list"])).lines().any(|l| l.starts_with("fake")), 10));

    let st = qoral(&env, &["stop"]);
    assert!(out(&st).contains("qoral stopped"), "{}", out(&st));
}

#[test]
fn mcp_tools_and_knowledge() {
    let env = setup();
    let proj = env.home.join("../proj2");
    std::fs::create_dir_all(&proj).unwrap();
    let run = |lines: &str| -> String {
        let mut child = Command::new(bin())
            .args(["mcp", "--agent", "tester"])
            .env("QORAL_HOME", &env.home)
            .current_dir(&proj)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(lines.as_bytes()).unwrap();
        String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).to_string()
    };
    let o = run(concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#, "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"qoral_send","arguments":{"to":"nobody","message":"x"}}}"#, "\n",
        r#"{"jsonrpc":"2.0","id":4,"method":"no/such"}"#, "\n",
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"qoral_remember","arguments":{"text":"CI runs on Node 26 only."}}}"#, "\n",
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"qoral_knowledge","arguments":{}}}"#, "\n",
    ));
    assert!(o.contains(r#""name":"qoral""#), "{o}");
    for t in ["qoral_send", "qoral_inbox", "qoral_wait", "qoral_spawn", "qoral_list_agents", "qoral_log", "qoral_remember", "qoral_knowledge", "qoral_whoami"] {
        assert!(o.contains(&format!(r#""name":"{t}""#)), "missing {t}");
    }
    assert!(o.contains("no active agent named \\\"nobody\\\""), "{o}");
    assert!(o.contains(r#""code":-32601"#), "{o}");
    assert!(o.contains("Saved to"), "{o}");
    assert!(o.contains("CI runs on Node 26 only"), "{o}");
    assert!(proj.join(".qoral/KNOWLEDGE.md").exists());
}
