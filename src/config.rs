use std::collections::HashMap;
use std::path::Path;

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
    pub run: Option<String>,
    pub build: Option<String>,
    pub image: Option<String>,
    #[serde(rename = "static")]
    pub static_dir: Option<String>,
    pub port: Option<u16>,
    pub health: Option<String>,
    pub volume: Option<String>,
    pub expose: Option<u16>,
    pub schedule: Option<String>,
    pub replicas: Option<Replicas>,
    #[serde(default)]
    pub after: Vec<String>,
    pub runtime: Option<String>,
    pub cluster: Option<bool>,
    pub team: Option<String>,
    pub spa: Option<bool>,
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
        }

        Ok(())
    }
}
