use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Result};
use tokio::process::Command;

use crate::config::Config;
use crate::env_file;
use crate::health;
use crate::history::{DeployOutcome, DeployRecorder};
use crate::runner::{self, default_port_for_image, register_env_vars};
use crate::snapshot;

pub async fn run(config: Config) -> Result<()> {
    let order = runner::toposort(&config.services)?;

    let mut env_vars: HashMap<String, String> = HashMap::new();
    let dotenv = env_file::load(Path::new(".env"))?;
    if !dotenv.is_empty() {
        println!("loaded {} vars from .env", dotenv.len());
        env_vars.extend(dotenv);
    }

    populate_service_env(&config, &mut env_vars);

    let mut recorder = DeployRecorder::start();
    println!("deploying {}...\n", config.app.name);

    let stateful: Vec<_> = config
        .services
        .iter()
        .filter(|s| snapshot::resolve_has_backup(s))
        .collect();

    if !stateful.is_empty() {
        println!("  snapshotting stateful services...");
        match snapshot::take_snapshot(&config.services, &config.app.name, &env_vars).await {
            Ok(meta) => {
                recorder.set_snapshot(&meta.id);
                for snap in &meta.services {
                    println!("    [ok] {} ({})", snap.name, snap.method);
                }
            }
            Err(e) => {
                recorder.migrate_fail("snapshot", &e.to_string());
                recorder.finish(DeployOutcome::Failed);
                recorder.save()?;
                bail!("snapshot failed, deploy aborted: {e}");
            }
        }
        println!();
    }

    let migrations: Vec<_> = order
        .iter()
        .filter_map(|name| {
            config
                .services
                .iter()
                .find(|s| s.name == *name && s.migrate.is_some())
        })
        .collect();

    if !migrations.is_empty() {
        println!("  running migrations...");
        for service in &migrations {
            let cmd = service.migrate.as_ref().unwrap();
            print!("    {} ... ", service.name);

            let output = Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .envs(&env_vars)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
                .await?;

            if output.status.success() {
                println!("ok");
                recorder.migrate_ok(&service.name);
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!("FAILED");
                eprintln!("    {}", stderr.trim());
                recorder.migrate_fail(&service.name, stderr.trim());

                if let Some(ref snapshot_id) = recorder.snapshot_id().map(String::from) {
                    println!("\n  migration failed, restoring snapshot {snapshot_id}...");
                    let meta = snapshot::load_snapshot(snapshot_id)?;
                    snapshot::restore_snapshot(&meta, &config.services, &config.app.name, &env_vars).await?;
                    recorder.rollback(&format!("restored snapshot {snapshot_id}"));
                    recorder.finish(DeployOutcome::RolledBack);
                    recorder.save()?;
                    bail!("migration failed for {}, rolled back to snapshot {snapshot_id}", service.name);
                }

                recorder.finish(DeployOutcome::Failed);
                recorder.save()?;
                bail!("migration failed for {}, no snapshot to restore", service.name);
            }
        }
        println!();
    }

    let restartable: Vec<_> = order
        .iter()
        .filter_map(|name| {
            config
                .services
                .iter()
                .find(|s| s.name == *name && s.schedule.is_none())
                .filter(|s| s.run.is_some() || s.build.is_some())
        })
        .collect();

    if !restartable.is_empty() {
        println!("  restarting services...");
        for service in &restartable {
            if service.image.is_some() {
                let container_name = format!("baton-{}-{}", config.app.name, service.name);
                restart_container(&container_name).await?;
                recorder.restart(&service.name);
                println!("    [ok] {} (container)", service.name);
            } else if service.run.is_some() || service.build.is_some() {
                recorder.restart(&service.name);
                println!("    [ok] {} (signalled)", service.name);
            }
        }
        println!();
    }

    let health_checks: Vec<_> = config
        .services
        .iter()
        .filter(|s| s.health.is_some() && s.port.is_some())
        .collect();

    if !health_checks.is_empty() {
        println!("  checking health...");
        for service in &health_checks {
            let port = service.port.unwrap();
            let path = service.health.as_ref().unwrap();
            print!("    {} :{}{} ... ", service.name, port, path);

            match health::wait_for_healthy(port, path).await {
                Ok(()) => {
                    println!("ok");
                    recorder.health_pass(&service.name);
                }
                Err(e) => {
                    println!("FAILED");
                    recorder.health_fail(&service.name, &e.to_string());

                    if let Some(ref snapshot_id) = recorder.snapshot_id().map(String::from) {
                        println!("\n  health check failed, restoring snapshot {snapshot_id}...");
                        let meta = snapshot::load_snapshot(snapshot_id)?;
                        snapshot::restore_snapshot(&meta, &config.services, &config.app.name, &env_vars).await?;
                        recorder.rollback(&format!("restored snapshot {snapshot_id} after health failure"));
                        recorder.finish(DeployOutcome::RolledBack);
                        recorder.save()?;
                        bail!(
                            "health check failed for {} after deploy, rolled back to snapshot {snapshot_id}",
                            service.name
                        );
                    }

                    recorder.finish(DeployOutcome::Failed);
                    recorder.save()?;
                    bail!("health check failed for {} after deploy", service.name);
                }
            }
        }
        println!();
    }

    recorder.finish(DeployOutcome::Success);
    recorder.save()?;

    println!("deploy complete.\n");
    Ok(())
}

fn populate_service_env(config: &Config, env_vars: &mut HashMap<String, String>) {
    for service in &config.services {
        let port = service
            .port
            .or(service.expose)
            .or_else(|| service.image.as_ref().and_then(|i| default_port_for_image(i)));
        if let Some(p) = port {
            register_env_vars(service, &config.app.name, p, env_vars);
        }
    }
}

async fn restart_container(name: &str) -> Result<()> {
    let output = Command::new("docker")
        .args(["restart", name])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("failed to restart container {}: {}", name, stderr.trim());
    }

    Ok(())
}
