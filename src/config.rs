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
}

impl Default for Config {
    fn default() -> Self {
        Self { summarizer: "auto".into(), model: None, document_sessions: true, max_agents: 8, theme: "default".into() }
    }
}

pub fn load() -> Config {
    std::fs::read_to_string(paths::home().join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
