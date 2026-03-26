mod config;
mod init;
mod runner;

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
        Command::Up { config, env: _env } => {
            let cfg = config::Config::load(&config)?;
            runner::run(cfg).await
        }
        Command::Status { config } => {
            let cfg = config::Config::load(&config)?;
            println!("{} status", cfg.app.name);
            for service in &cfg.services {
                println!("  {}  not running", service.name);
            }
            Ok(())
        }
        Command::Add { service_type, name, port, run, schedule } => {
            println!("would add {service_type} service");
            if let Some(n) = &name {
                println!("  name: {n}");
            }
            if let Some(p) = port {
                println!("  port: {p}");
            }
            if let Some(r) = &run {
                println!("  run: {r}");
            }
            if let Some(s) = &schedule {
                println!("  schedule: {s}");
            }
            Ok(())
        }
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
