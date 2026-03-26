mod add;
mod agent;
mod build;
mod chaos;
mod config;
mod cron;
mod env_file;
mod health;
mod init;
mod proxy;
mod runner;
mod server;
mod static_server;

use std::path::PathBuf;
use std::time::Duration;

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
        chaos: bool,
        #[arg(long, default_value = "30")]
        chaos_interval: u64,
        #[arg(long, default_value = "0.3")]
        chaos_probability: f64,
        #[arg(long)]
        chaos_target: Option<String>,
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
    Node {
        #[command(subcommand)]
        action: NodeAction,
    },
    Server {
        #[arg(long, default_value = "baton.toml")]
        config: PathBuf,
        #[arg(long, default_value = "9090")]
        port: u16,
    },
    Agent {
        #[arg(long)]
        server: String,
        #[arg(long, default_value = "9091")]
        port: u16,
    },
}

#[derive(Subcommand)]
enum NodeAction {
    Add { addresses: Vec<String> },
    List,
    Remove { address: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init => init::run().await,
        Command::Up {
            config,
            env: _env,
            chaos: enable_chaos,
            chaos_interval,
            chaos_probability,
            chaos_target,
        } => {
            let cfg = config::Config::load(&config)?;
            let chaos_cfg = if enable_chaos {
                Some(chaos::ChaosConfig {
                    kill_interval: Duration::from_secs(chaos_interval),
                    kill_probability: chaos_probability,
                    target: chaos_target,
                })
            } else {
                None
            };
            runner::run(cfg, chaos_cfg).await
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
            println!("{} status", cfg.app.name);
            for service in &cfg.services {
                println!("  {}  not running", service.name);
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
        Command::Node { action } => {
            match action {
                NodeAction::Add { addresses } => {
                    for addr in &addresses {
                        println!("would install agent on {addr}");
                    }
                }
                NodeAction::List => println!("no nodes configured"),
                NodeAction::Remove { address } => println!("would remove {address}"),
            }
            Ok(())
        }
        Command::Server { config, port } => {
            let cfg = config::Config::load(&config)?;
            server::run(cfg, port).await
        }
        Command::Agent { server: server_addr, port } => {
            agent::run(server_addr, port).await
        }
    }
}
