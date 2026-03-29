mod add;
mod build;
mod config;
mod cron;
mod dashboard;
mod deploy;
mod env_file;
mod health;
mod history;
mod init;
mod proxy;
mod runner;
mod snapshot;
mod static_server;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "baton", version, about = "Deploy apps, not infrastructure")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init,
    Up {
        #[arg(long, default_value = "baton.toml")]
        config: PathBuf,
        #[arg(long)]
        env: Option<String>,
        #[arg(long)]
        ui: bool,
        #[arg(long, default_value = "9500")]
        ui_port: u16,
    },
    Deploy {
        #[arg(long, default_value = "baton.toml")]
        config: PathBuf,
        #[arg(long)]
        env: Option<String>,
    },
    Rollback {
        #[arg(long, default_value = "baton.toml")]
        config: PathBuf,
        snapshot_id: Option<String>,
    },
    History {
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    Snapshot {
        #[arg(long, default_value = "baton.toml")]
        config: PathBuf,
    },
    Restore {
        #[arg(long, default_value = "baton.toml")]
        config: PathBuf,
        snapshot_id: String,
    },
    Validate {
        #[arg(long, default_value = "baton.toml")]
        config: PathBuf,
    },
    Status {
        #[arg(long, default_value = "baton.toml")]
        config: PathBuf,
    },
    Add {
        service_type: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        run: Option<String>,
        #[arg(long)]
        schedule: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init => init::run().await,
        Command::Up {
            config,
            env,
            ui: enable_ui,
            ui_port,
        } => {
            let mut cfg = config::Config::load(&config)?;
            if let Some(ref env_name) = env {
                apply_environment(&mut cfg, env_name)?;
                println!("using environment: {env_name}");
            }
            let ui_cfg = if enable_ui { Some(ui_port) } else { None };
            runner::run(cfg, ui_cfg).await
        }
        Command::Deploy { config, env } => {
            let mut cfg = config::Config::load(&config)?;
            if let Some(ref env_name) = env {
                apply_environment(&mut cfg, env_name)?;
                println!("using environment: {env_name}");
            }
            deploy::run(cfg).await
        }
        Command::Rollback {
            config,
            snapshot_id,
        } => {
            let cfg = config::Config::load(&config)?;
            let meta = match snapshot_id {
                Some(id) => snapshot::load_snapshot(&id)?,
                None => snapshot::latest_snapshot()?
                    .ok_or_else(|| anyhow::anyhow!("no snapshots found"))?,
            };
            println!("restoring snapshot {}...\n", meta.id);
            let env_vars = load_env(&cfg)?;
            snapshot::restore_snapshot(&meta, &cfg.services, &cfg.app.name, &env_vars).await?;
            for snap in &meta.services {
                println!("  [ok] {} restored ({})", snap.name, snap.method);
            }
            println!("\nrollback complete.");
            Ok(())
        }
        Command::History { limit } => {
            let records = history::load_history()?;
            if records.is_empty() {
                println!("no deploy history found.");
            } else {
                println!("deploy history\n");
                history::print_history(&records, limit);
            }
            Ok(())
        }
        Command::Snapshot { config } => {
            let cfg = config::Config::load(&config)?;
            let env_vars = load_env(&cfg)?;
            println!("taking snapshot...\n");
            let meta =
                snapshot::take_snapshot(&cfg.services, &cfg.app.name, &env_vars).await?;
            for snap in &meta.services {
                println!("  [ok] {} ({})", snap.name, snap.method);
            }
            println!("\nsnapshot {} saved.", meta.id);
            Ok(())
        }
        Command::Restore {
            config,
            snapshot_id,
        } => {
            let cfg = config::Config::load(&config)?;
            let meta = snapshot::load_snapshot(&snapshot_id)?;
            let env_vars = load_env(&cfg)?;
            println!("restoring snapshot {}...\n", meta.id);
            snapshot::restore_snapshot(&meta, &cfg.services, &cfg.app.name, &env_vars).await?;
            for snap in &meta.services {
                println!("  [ok] {} restored ({})", snap.name, snap.method);
            }
            println!("\nrestore complete.");
            Ok(())
        }
        Command::Validate { config } => {
            let cfg = config::Config::load(&config)?;
            println!("{} is valid", config.display());
            println!("  app: {}", cfg.app.name);
            if let Some(domain) = &cfg.app.domain {
                println!("  domain: {domain}");
            }
            println!("  services: {}", cfg.services.len());
            for svc in &cfg.services {
                let kind = if svc.image.is_some() {
                    "container"
                } else if svc.build.is_some() {
                    "build"
                } else if svc.static_dir.is_some() {
                    "static"
                } else if svc.schedule.is_some() {
                    "cron"
                } else {
                    "process"
                };
                println!("    {} ({})", svc.name, kind);
            }
            if !cfg.environments.is_empty() {
                println!(
                    "  environments: {}",
                    cfg.environments
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            Ok(())
        }
        Command::Status { config } => {
            let cfg = config::Config::load(&config)?;
            println!("{} status\n", cfg.app.name);
            for service in &cfg.services {
                let status = check_service_status(service, &cfg.app.name).await;
                println!("  {}  {}", service.name, status);
            }
            Ok(())
        }
        Command::Add {
            service_type,
            name,
            port,
            run,
            schedule,
        } => add::run(add::AddOptions {
            service_type,
            name,
            port,
            run,
            schedule,
        }),
    }
}

fn apply_environment(cfg: &mut config::Config, env_name: &str) -> Result<()> {
    let environment = cfg
        .environments
        .get(env_name)
        .ok_or_else(|| anyhow::anyhow!("environment '{}' not found in config", env_name))?
        .clone();
    if let Some(domain) = environment.domain {
        cfg.app.domain = Some(domain);
    }
    Ok(())
}

fn load_env(cfg: &config::Config) -> Result<std::collections::HashMap<String, String>> {
    let mut env_vars = std::collections::HashMap::new();
    let dotenv = env_file::load(std::path::Path::new(".env"))?;
    env_vars.extend(dotenv);
    for service in &cfg.services {
        let port = service
            .port
            .or(service.expose)
            .or_else(|| {
                service
                    .image
                    .as_ref()
                    .and_then(|i| runner::default_port_for_image(i))
            });
        if let Some(p) = port {
            runner::register_env_vars(service, &cfg.app.name, p, &mut env_vars);
        }
    }
    Ok(env_vars)
}

async fn check_service_status(service: &config::Service, app_name: &str) -> String {
    if service.image.is_some() || service.build.is_some() {
        let container_name = format!("baton-{}-{}", app_name, service.name);
        if is_container_running(&container_name).await {
            return "running (container)".to_string();
        }
        return "stopped".to_string();
    }

    if service.schedule.is_some() {
        return "scheduled".to_string();
    }

    if let Some(port) = service.port {
        if tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .is_ok()
        {
            return format!("running (:{port})");
        }
        return "stopped".to_string();
    }

    "unknown".to_string()
}

async fn is_container_running(name: &str) -> bool {
    let output = tokio::process::Command::new("docker")
        .args(["inspect", "-f", "{{.State.Running}}", name])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim() == "true",
        _ => {
            let output = tokio::process::Command::new("podman")
                .args(["inspect", "-f", "{{.State.Running}}", name])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .output()
                .await;
            match output {
                Ok(o) if o.status.success() => {
                    String::from_utf8_lossy(&o.stdout).trim() == "true"
                }
                _ => false,
            }
        }
    }
}
