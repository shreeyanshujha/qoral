//! ~/.local/share/qoral/config.json
use serde::Deserialize;

use crate::paths;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub summarizer: String,
    pub model: Option<String>,
    pub document_sessions: bool,
    pub max_agents: usize,
    pub theme: String,
    /// Harness pool for debates when --agents isn't given, e.g. ["claude", "agy"]. Empty = auto-detect.
    pub debate_harnesses: Vec<String>,
    /// Default number of debate participants when neither --count nor --agents is given.
    pub debate_count: usize,
    /// Model for OpenCode agents as provider/model, e.g. "deepseek/deepseek-chat". None = OpenCode's default.
    pub opencode_model: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            summarizer: "auto".into(),
            model: None,
            document_sessions: true,
            max_agents: 8,
            theme: "default".into(),
            debate_harnesses: vec![],
            debate_count: 3,
            opencode_model: None,
        }
    }
}

pub fn load() -> Config {
    std::fs::read_to_string(paths::home().join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
