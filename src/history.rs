use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

const BATON_DIR: &str = ".baton";
const HISTORY_FILE: &str = "history.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployRecord {
    pub id: String,
    pub started: String,
    pub finished: Option<String>,
    pub outcome: DeployOutcome,
    pub snapshot_id: Option<String>,
    pub events: Vec<DeployEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeployOutcome {
    Success,
    Failed,
    RolledBack,
    InProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployEvent {
    pub timestamp: String,
    pub kind: EventKind,
    pub service: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    DeployStart,
    Snapshot,
    Migrate,
    MigrateFail,
    Restart,
    HealthPass,
    HealthFail,
    Rollback,
    DeployComplete,
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeployStart => write!(f, "deploy start"),
            Self::Snapshot => write!(f, "snapshot"),
            Self::Migrate => write!(f, "migrate"),
            Self::MigrateFail => write!(f, "migrate failed"),
            Self::Restart => write!(f, "restart"),
            Self::HealthPass => write!(f, "health pass"),
            Self::HealthFail => write!(f, "health failed"),
            Self::Rollback => write!(f, "rollback"),
            Self::DeployComplete => write!(f, "deploy complete"),
        }
    }
}

fn history_path() -> PathBuf {
    PathBuf::from(BATON_DIR).join(HISTORY_FILE)
}

pub struct DeployRecorder {
    record: DeployRecord,
}

impl DeployRecorder {
    pub fn start() -> Self {
        let id = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let record = DeployRecord {
            id,
            started: Utc::now().to_rfc3339(),
            finished: None,
            outcome: DeployOutcome::InProgress,
            snapshot_id: None,
            events: vec![DeployEvent {
                timestamp: Utc::now().to_rfc3339(),
                kind: EventKind::DeployStart,
                service: None,
                detail: "deploy started".to_string(),
            }],
        };
        Self { record }
    }

    pub fn set_snapshot(&mut self, snapshot_id: &str) {
        self.record.snapshot_id = Some(snapshot_id.to_string());
        self.push(EventKind::Snapshot, None, format!("snapshot {snapshot_id}"));
    }

    pub fn migrate_ok(&mut self, service: &str) {
        self.push(EventKind::Migrate, Some(service), "migration succeeded".to_string());
    }

    pub fn migrate_fail(&mut self, service: &str, err: &str) {
        self.push(EventKind::MigrateFail, Some(service), err.to_string());
    }

    pub fn restart(&mut self, service: &str) {
        self.push(EventKind::Restart, Some(service), "restarted".to_string());
    }

    pub fn health_pass(&mut self, service: &str) {
        self.push(EventKind::HealthPass, Some(service), "healthy".to_string());
    }

    pub fn health_fail(&mut self, service: &str, err: &str) {
        self.push(EventKind::HealthFail, Some(service), err.to_string());
    }

    pub fn rollback(&mut self, detail: &str) {
        self.push(EventKind::Rollback, None, detail.to_string());
    }

    pub fn finish(&mut self, outcome: DeployOutcome) {
        self.record.outcome = outcome;
        self.record.finished = Some(Utc::now().to_rfc3339());
        self.push(EventKind::DeployComplete, None, format!("{:?}", self.record.outcome));
    }

    pub fn snapshot_id(&self) -> Option<&str> {
        self.record.snapshot_id.as_deref()
    }

    pub fn save(&self) -> Result<()> {
        let path = history_path();
        std::fs::create_dir_all(BATON_DIR)?;

        let mut records = load_history().unwrap_or_default();
        records.push(self.record.clone());

        let json = serde_json::to_string_pretty(&records)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    fn push(&mut self, kind: EventKind, service: Option<&str>, detail: String) {
        self.record.events.push(DeployEvent {
            timestamp: Utc::now().to_rfc3339(),
            kind,
            service: service.map(String::from),
            detail,
        });
    }
}

pub fn load_history() -> Result<Vec<DeployRecord>> {
    let path = history_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    let records: Vec<DeployRecord> = serde_json::from_str(&content)?;
    Ok(records)
}

pub fn print_history(records: &[DeployRecord], limit: usize) {
    let start = if records.len() > limit { records.len() - limit } else { 0 };
    for record in &records[start..] {
        let outcome = match record.outcome {
            DeployOutcome::Success => "ok",
            DeployOutcome::Failed => "FAILED",
            DeployOutcome::RolledBack => "ROLLED BACK",
            DeployOutcome::InProgress => "in progress",
        };
        println!("  {} [{}] {}", record.id, outcome, record.started);

        for event in &record.events {
            let svc = event.service.as_deref().unwrap_or("");
            if svc.is_empty() {
                println!("    {} {}", event.kind, event.detail);
            } else {
                println!("    {} {} {}", event.kind, svc, event.detail);
            }
        }
        println!();
    }
}
