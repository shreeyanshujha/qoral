//! Heuristic status detection from the last screenful of an agent's terminal.
//! Statuses: starting | working | idle | attention | exited | documenting
use regex::Regex;
use std::sync::LazyLock;

static EXITED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\[qoral\] agent "[^"]+" exited"#).unwrap());
static DOCUMENTING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[qoral\] documenting session").unwrap());
static WORKING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(esc to interrupt|esc to cancel|ctrl\+c to (cancel|interrupt|stop)|Thinking\.\.\.|Working\.\.\.|Generating\.\.\.)").unwrap()
});
static ATTENTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(Do you want to|Allow (this|execution|once|always)|Would you like to|\(y/n\)|\[Y/n\]|\[y/N\]|Yes, (allow|proceed|and don't ask|I trust|and always)|Yes, run|approve this|Enter to confirm|Press Enter to continue|trust this folder|trust the contents|Select (an option|login method)|Paste (your|the) (code|key)|Sign in|Log ?in with|No, exit|Requesting permission)").unwrap()
});
static PROMPT_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[\s│┃|]*[>❯›]\s").unwrap());
static PROMPT_HINT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\? for shortcuts|Type your message|shift\+tab to cycle|Ctrl\+C to quit|for commands)").unwrap()
});

pub fn detect_status(screen: &str) -> &'static str {
    let lines: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return "starting";
    }
    let start = lines.len().saturating_sub(16);
    let tail = lines[start..].join("\n");
    if EXITED.is_match(&tail) {
        return "exited";
    }
    if DOCUMENTING.is_match(&tail) {
        return "documenting";
    }
    if ATTENTION.is_match(&tail) {
        return "attention";
    }
    if WORKING.is_match(&tail) {
        return "working";
    }
    // Prompt detection: a prompt glyph at the start of a line, possibly followed by a newline
    // (trailing spaces are trimmed on capture), or a known idle hint.
    let tail_nl = format!("{tail}\n");
    if PROMPT_LINE.is_match(&tail_nl) || PROMPT_HINT.is_match(&tail) {
        return "idle";
    }
    "working"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses() {
        assert_eq!(detect_status("✻ done\n❯ \n  ? for shortcuts"), "idle");
        assert_eq!(detect_status("● Calling…\n❯ \n  esc to interrupt"), "working");
        assert_eq!(detect_status("Do you want to proceed?\n❯ 1. Yes\n  2. No"), "attention");
        assert_eq!(detect_status("Do you trust the contents of this project?\n> Yes, I trust this folder"), "attention");
        assert_eq!(detect_status("Requesting permission for:\n  ls\nDo you want to proceed?"), "attention");
        assert_eq!(detect_status("\n[qoral] agent \"ada\" exited (0)."), "exited");
        assert_eq!(detect_status("\n\n"), "starting");
        assert_eq!(detect_status("⡿ Generating...\n>\nesc to cancel"), "working");
    }
}
