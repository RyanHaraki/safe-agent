use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub at: u64,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    pub session_id: String,
    pub workspace: PathBuf,
    pub policy_hash: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub events: Vec<AuditEvent>,
}

impl AuditLog {
    pub fn new(session_id: String, workspace: PathBuf, policy_hash: String) -> Self {
        Self {
            session_id,
            workspace,
            policy_hash,
            started_at: now(),
            finished_at: None,
            events: vec![],
        }
    }
    pub fn record_request(&mut self, request: &impl std::fmt::Debug, decision: &str, detail: &str) {
        self.events.push(AuditEvent {
            at: now(),
            kind: format!("request:{decision}"),
            detail: format!("{request:?}: {detail}"),
        });
    }
    pub fn finish(&mut self) {
        self.finished_at = Some(now());
    }
}

pub fn durable_dir() -> PathBuf {
    if let Ok(path) = std::env::var("SAFE_AGENT_TEST_CONFIG_HOME") {
        return PathBuf::from(path).join("sessions");
    }
    crate::config::config_dir().join("sessions")
}
pub fn durable_session_path(id: &str) -> PathBuf {
    durable_dir().join(format!("{id}.json"))
}
pub fn latest_path() -> PathBuf {
    durable_dir().join("latest")
}

pub fn summary(id: Option<&str>, json: bool) -> anyhow::Result<()> {
    let selected = id.map(str::to_owned).or_else(|| {
        fs::read_to_string(latest_path())
            .ok()
            .map(|s| s.trim().to_owned())
    });
    let Some(id) = selected else {
        println!("No Safe Agent sessions found.");
        return Ok(());
    };
    let path = durable_session_path(&id);
    let log: AuditLog = serde_json::from_slice(&fs::read(&path)?)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&log)?);
        return Ok(());
    }
    println!("Session: {}", log.session_id);
    println!("Workspace: {}", log.workspace.display());
    println!("Policy hash: {}", log.policy_hash);
    println!("Events: {}", log.events.len());
    for event in log.events {
        println!("  [{}] {} {}", event.at, event.kind, event.detail);
    }
    println!("Changes are measured from session start and may include concurrent user edits.");
    Ok(())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
