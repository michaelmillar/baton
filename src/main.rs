mod add;
mod chaos;
mod config;
mod cron;
mod init;
mod runner;
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
    Server,
    Agent,
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
        Command::Server => {
            println!("baton server starting...");
            Ok(())
        }
        Command::Agent => {
            println!("baton agent starting...");
            Ok(())
        }
    }
}
