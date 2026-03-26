use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub app: App,
    #[serde(default)]
    pub environments: HashMap<String, Environment>,
    #[serde(default, rename = "service")]
    pub services: Vec<Service>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct App {
    pub name: String,
    pub domain: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Environment {
    pub domain: Option<String>,
    #[serde(default)]
    pub nodes: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Service {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(rename = "static", skip_serializing_if = "Option::is_none")]
    pub static_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expose: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replicas: Option<Replicas>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spa: Option<bool>,
}

impl Service {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            run: None,
            build: None,
            image: None,
            static_dir: None,
            port: None,
            health: None,
            volume: None,
            expose: None,
            schedule: None,
            replicas: None,
            after: vec![],
            runtime: None,
            cluster: None,
            team: None,
            spa: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Replicas {
    Count(u32),
    PerEnvironment(HashMap<String, u32>),
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let config: Config =
            toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        for service in &self.services {
            let has_source =
                service.run.is_some()
                    || service.build.is_some()
                    || service.image.is_some()
                    || service.static_dir.is_some();

            anyhow::ensure!(
                has_source,
                "service '{}' must have one of: run, build, image, or static",
                service.name
            );

            for dep in &service.after {
                let exists = self.services.iter().any(|s| s.name == *dep);
                anyhow::ensure!(
                    exists,
                    "service '{}' depends on '{}' which does not exist",
                    service.name,
                    dep
                );
            }

            if let Some(schedule) = &service.schedule {
                let expr = normalise_cron(schedule);
                cron::Schedule::from_str(&expr)
                    .map_err(|e| anyhow::anyhow!("service '{}' has invalid schedule '{}': {}", service.name, schedule, e))?;
            }
        }

        Ok(())
    }
}

pub fn normalise_cron(expr: &str) -> String {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    match fields.len() {
        5 => format!("0 {expr} *"),
        6 => format!("0 {expr}"),
        _ => expr.to_string(),
    }
}
