use std::path::Path;

use anyhow::{Context, Result};

use crate::config::{App, Config, Service};

pub async fn run() -> Result<()> {
    let config_path = Path::new("baton.toml");

    if config_path.exists() {
        anyhow::bail!("baton.toml already exists");
    }

    let dir_name = std::env::current_dir()?
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "app".to_string());

    let mut services = Vec::new();

    if Path::new("Dockerfile").exists() {
        println!("detected Dockerfile");
        services.push(Service {
            name: "web".to_string(),
            build: Some(".".to_string()),
            run: None,
            image: None,
            static_dir: None,
            port: Some(8080),
            health: Some("/health".to_string()),
            volume: None,
            expose: None,
            schedule: None,
            replicas: None,
            after: vec![],
            runtime: None,
            cluster: None,
            team: None,
            spa: None,
        });
    } else if Path::new("Cargo.toml").exists() {
        println!("detected Rust project");
        services.push(Service {
            name: "web".to_string(),
            run: Some(format!("./target/release/{dir_name}")),
            build: None,
            image: None,
            static_dir: None,
            port: Some(8080),
            health: Some("/health".to_string()),
            volume: None,
            expose: None,
            schedule: None,
            replicas: None,
            after: vec![],
            runtime: None,
            cluster: None,
            team: None,
            spa: None,
        });
    } else if Path::new("mix.exs").exists() {
        println!("detected Elixir project");
        services.push(Service {
            name: "web".to_string(),
            build: Some(".".to_string()),
            run: None,
            image: None,
            static_dir: None,
            port: Some(4000),
            health: Some("/health".to_string()),
            volume: None,
            expose: None,
            schedule: None,
            replicas: None,
            after: vec![],
            runtime: Some("beam".to_string()),
            cluster: None,
            team: None,
            spa: None,
        });
    } else if Path::new("package.json").exists() {
        println!("detected Node.js project");
        services.push(Service {
            name: "web".to_string(),
            build: Some(".".to_string()),
            run: None,
            image: None,
            static_dir: None,
            port: Some(3000),
            health: Some("/health".to_string()),
            volume: None,
            expose: None,
            schedule: None,
            replicas: None,
            after: vec![],
            runtime: None,
            cluster: None,
            team: None,
            spa: None,
        });
    } else if Path::new("go.mod").exists() {
        println!("detected Go project");
        services.push(Service {
            name: "web".to_string(),
            run: Some(format!("./{dir_name}")),
            build: None,
            image: None,
            static_dir: None,
            port: Some(8080),
            health: Some("/health".to_string()),
            volume: None,
            expose: None,
            schedule: None,
            replicas: None,
            after: vec![],
            runtime: None,
            cluster: None,
            team: None,
            spa: None,
        });
    } else {
        println!("no project detected, generating minimal config");
    }

    let config = Config {
        app: App {
            name: dir_name.clone(),
            domain: None,
        },
        environments: Default::default(),
        services,
    };

    let toml_str = toml::to_string_pretty(&config)
        .context("failed to serialise config")?;

    std::fs::write(config_path, &toml_str)
        .context("failed to write orchestra.toml")?;

    println!("created baton.toml for '{dir_name}'");
    println!("edit it, then run: baton up");

    Ok(())
}
