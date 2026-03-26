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
        let mut svc = Service::new("web");
        svc.build = Some(".".to_string());
        svc.port = Some(8080);
        svc.health = Some("/health".to_string());
        services.push(svc);
    } else if Path::new("Cargo.toml").exists() {
        println!("detected Rust project");
        let mut svc = Service::new("web");
        svc.run = Some(format!("./target/release/{dir_name}"));
        svc.port = Some(8080);
        svc.health = Some("/health".to_string());
        services.push(svc);
    } else if Path::new("mix.exs").exists() {
        println!("detected Elixir project");
        let mut svc = Service::new("web");
        svc.build = Some(".".to_string());
        svc.port = Some(4000);
        svc.health = Some("/health".to_string());
        svc.runtime = Some("beam".to_string());
        services.push(svc);
    } else if Path::new("package.json").exists() {
        println!("detected Node.js project");
        let mut svc = Service::new("web");
        svc.build = Some(".".to_string());
        svc.port = Some(3000);
        svc.health = Some("/health".to_string());
        services.push(svc);
    } else if Path::new("go.mod").exists() {
        println!("detected Go project");
        let mut svc = Service::new("web");
        svc.run = Some(format!("./{dir_name}"));
        svc.port = Some(8080);
        svc.health = Some("/health".to_string());
        services.push(svc);
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
        .context("failed to write baton.toml")?;

    println!("created baton.toml for '{dir_name}'");
    println!("edit it, then run: baton up");

    Ok(())
}
