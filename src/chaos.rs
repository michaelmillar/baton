use std::process::Stdio;
use std::time::Duration;

use anyhow::{bail, Result};
use rand::Rng;
use tokio::process::Command;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct ChaosConfig {
    pub kill_interval: Duration,
    pub kill_probability: f64,
    pub target: Option<String>,
}

impl Default for ChaosConfig {
    fn default() -> Self {
        Self {
            kill_interval: Duration::from_secs(30),
            kill_probability: 0.3,
            target: None,
        }
    }
}

pub fn spawn_chaos_monkey(
    app_name: String,
    services: Vec<String>,
    chaos_cfg: ChaosConfig,
    container_runtime: Option<String>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        println!("[chaos] armed. interval={}s probability={:.0}%",
            chaos_cfg.kill_interval.as_secs(),
            chaos_cfg.kill_probability * 100.0
        );

        let targets: Vec<&String> = if let Some(ref name) = chaos_cfg.target {
            services.iter().filter(|s| *s == name).collect()
        } else {
            services.iter().collect()
        };

        if targets.is_empty() {
            eprintln!("[chaos] no matching services to target");
            return;
        }

        loop {
            tokio::select! {
                _ = tokio::time::sleep(chaos_cfg.kill_interval) => {}
                _ = shutdown_rx.changed() => break,
            }

            if *shutdown_rx.borrow() {
                break;
            }

            let (should_fire, idx) = {
                let mut rng = rand::rng();
                let fire = rng.random::<f64>() <= chaos_cfg.kill_probability;
                let i = rng.random_range(0..targets.len());
                (fire, i)
            };

            if !should_fire {
                continue;
            }

            let victim = targets[idx];

            println!("[chaos] killing '{victim}'");

            if let Some(ref rt) = container_runtime {
                let container_name = format!("baton-{app_name}-{victim}");
                let _ = Command::new(rt)
                    .args(["kill", &container_name])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await;
            }

            let _ = Command::new("pkill")
                .args(["-f", &format!("baton.*{victim}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }

        println!("[chaos] disarmed");
    })
}

pub fn validate_chaos_targets(config: &Config, target: &Option<String>) -> Result<Vec<String>> {
    let service_names: Vec<String> = config.services.iter().map(|s| s.name.clone()).collect();

    if let Some(name) = target {
        if !service_names.contains(name) {
            bail!("chaos target '{}' not found in services", name);
        }
    }

    Ok(service_names)
}
