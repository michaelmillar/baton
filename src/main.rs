mod add;
mod build;
mod config;
mod cron;
mod dashboard;
mod env_file;
mod health;
mod init;
mod proxy;
mod runner;
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
                let environment = cfg.environments.get(env_name)
                    .ok_or_else(|| anyhow::anyhow!("environment '{}' not found in config", env_name))?
                    .clone();
                if let Some(domain) = environment.domain {
                    cfg.app.domain = Some(domain);
                }
                println!("using environment: {env_name}");
            }
            let ui_cfg = if enable_ui {
                Some(ui_port)
            } else {
                None
            };
            runner::run(cfg, ui_cfg).await
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
                println!("  environments: {}", cfg.environments.keys().cloned().collect::<Vec<_>>().join(", "));
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
        if tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")).await.is_ok() {
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
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim() == "true"
        }
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
