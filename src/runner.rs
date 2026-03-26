use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{bail, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::build;
use crate::chaos::{self, ChaosConfig};
use crate::config::{Config, Service};
use crate::cron::spawn_cron_task;
use crate::env_file;
use crate::health;
use crate::proxy::{self, ProxyRoute};
use crate::static_server::spawn_static_server;

enum ServiceHandle {
    Process { task: JoinHandle<()> },
    Container { runtime: String, container_name: String },
    Static { task: JoinHandle<()> },
    Cron { task: JoinHandle<()> },
    Proxy { task: JoinHandle<()> },
}

pub async fn run(config: Config, chaos_cfg: Option<ChaosConfig>) -> Result<()> {
    let order = toposort(&config.services)?;

    let needs_containers = config.services.iter().any(|s| s.image.is_some() || s.build.is_some());
    let container_runtime = if needs_containers {
        Some(detect_container_runtime().await?)
    } else {
        None
    };

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut handles: Vec<(String, ServiceHandle)> = Vec::new();
    let mut env_vars: HashMap<String, String> = HashMap::new();
    let mut proxy_routes: Vec<ProxyRoute> = Vec::new();

    let dotenv = env_file::load(Path::new(".env"))?;
    if !dotenv.is_empty() {
        println!("loaded {} vars from .env", dotenv.len());
        env_vars.extend(dotenv);
    }

    println!("starting {}...\n", config.app.name);

    for service_name in &order {
        let service = config
            .services
            .iter()
            .find(|s| s.name == *service_name)
            .unwrap();

        if let Some(build_ctx) = &service.build {
            let rt = container_runtime.as_ref().unwrap();
            let image_tag = format!("baton-{}-{}", config.app.name, service.name);
            build::build_image(rt, &image_tag, build_ctx).await?;

            let container_name = format!("baton-{}-{}", config.app.name, service.name);
            let port = service.port.or(service.expose);

            let build_svc = Service {
                image: Some(image_tag.clone()),
                ..Service::new(&service.name)
            };

            start_container(rt, &container_name, &image_tag, &build_svc, &config.app.name, port).await?;

            if let Some(p) = port {
                if let Some(health_path) = &service.health {
                    health::wait_for_healthy(p, health_path).await?;
                } else {
                    health::wait_for_port(p).await?;
                }
                register_env_vars(service, &config.app.name, p, &mut env_vars);
                collect_proxy_route(&config, service, p, &mut proxy_routes);
            }

            println!("  [ok] {}  built and running on :{}", service.name, port.unwrap_or(0));

            handles.push((
                service.name.clone(),
                ServiceHandle::Container {
                    runtime: rt.clone(),
                    container_name,
                },
            ));
        } else if let Some(image) = &service.image {
            let rt = container_runtime.as_ref().unwrap();
            let container_name = format!("baton-{}-{}", config.app.name, service.name);
            let port = service
                .port
                .or(service.expose)
                .or_else(|| default_port_for_image(image));

            start_container(rt, &container_name, image, service, &config.app.name, port).await?;

            if let Some(p) = port {
                health::wait_for_port(p).await?;
                register_env_vars(service, &config.app.name, p, &mut env_vars);
            }

            println!("  [ok] {}  {} on :{}", service.name, image, port.unwrap_or(0));

            handles.push((
                service.name.clone(),
                ServiceHandle::Container {
                    runtime: rt.clone(),
                    container_name,
                },
            ));
        } else if let Some(cmd) = &service.run {
            if let Some(schedule) = &service.schedule {
                let task = spawn_cron_task(
                    service.name.clone(),
                    cmd.clone(),
                    schedule.clone(),
                    env_vars.clone(),
                    shutdown_rx.clone(),
                );

                println!("  [ok] {}  {} scheduled ({})", service.name, cmd, schedule);
                handles.push((service.name.clone(), ServiceHandle::Cron { task }));
            } else {
                let task = spawn_process(
                    service.name.clone(),
                    cmd.clone(),
                    env_vars.clone(),
                    shutdown_rx.clone(),
                );

                if let Some(port) = service.port {
                    if let Some(health_path) = &service.health {
                        health::wait_for_healthy(port, health_path).await?;
                    } else {
                        health::wait_for_port(port).await?;
                    }
                    register_env_vars(service, &config.app.name, port, &mut env_vars);
                    collect_proxy_route(&config, service, port, &mut proxy_routes);
                    println!("  [ok] {}  {} on :{}", service.name, cmd, port);
                } else {
                    println!("  [ok] {}  {} running", service.name, cmd);
                }

                handles.push((service.name.clone(), ServiceHandle::Process { task }));
            }
        } else if let Some(dir) = &service.static_dir {
            let port = service.port.unwrap_or(3000);
            let spa = service.spa.unwrap_or(false);
            let dir_path = std::path::PathBuf::from(dir);

            if !dir_path.exists() {
                bail!("static directory '{}' does not exist", dir);
            }

            let task = spawn_static_server(
                service.name.clone(),
                dir_path,
                port,
                spa,
                shutdown_rx.clone(),
            );

            health::wait_for_port(port).await?;
            collect_proxy_route(&config, service, port, &mut proxy_routes);
            let mode = if spa { "spa" } else { "static" };
            println!("  [ok] {}  {} ({}) on :{}", service.name, dir, mode, port);
            handles.push((service.name.clone(), ServiceHandle::Static { task }));
        }
    }

    if !proxy_routes.is_empty() {
        let proxy_port = 80;
        let task = proxy::spawn_proxy(proxy_routes, proxy_port, shutdown_rx.clone());
        handles.push(("proxy".to_string(), ServiceHandle::Proxy { task }));
    }

    if let Some(ref cc) = chaos_cfg {
        let service_names = chaos::validate_chaos_targets(&config, &cc.target)?;
        let chaos_handle = chaos::spawn_chaos_monkey(
            config.app.name.clone(),
            service_names,
            cc.clone(),
            container_runtime.clone(),
            shutdown_rx.clone(),
        );
        handles.push(("chaos".to_string(), ServiceHandle::Process { task: chaos_handle }));
    }

    println!("\nall services running. ctrl+c to stop.\n");

    tokio::signal::ctrl_c().await?;
    println!("\nshutting down...");

    let _ = shutdown_tx.send(true);

    for (name, handle) in handles.iter().rev() {
        match handle {
            ServiceHandle::Container {
                runtime,
                container_name,
            } => {
                stop_container(runtime, container_name).await;
            }
            _ => {}
        }
        println!("  stopped {name}");
    }

    for (_, handle) in handles {
        match handle {
            ServiceHandle::Process { task }
            | ServiceHandle::Static { task }
            | ServiceHandle::Cron { task }
            | ServiceHandle::Proxy { task } => {
                task.abort();
                let _ = task.await;
            }
            ServiceHandle::Container { .. } => {}
        }
    }

    println!("done.");
    Ok(())
}

fn collect_proxy_route(
    config: &Config,
    service: &Service,
    port: u16,
    routes: &mut Vec<ProxyRoute>,
) {
    let app_domain = match &config.app.domain {
        Some(d) => d,
        None => return,
    };

    let domain = format!("{}.{}", service.name, app_domain);
    routes.push(ProxyRoute {
        domain,
        backend: SocketAddr::from(([127, 0, 0, 1], port)),
    });
}

fn spawn_process(
    name: String,
    cmd: String,
    env: HashMap<String, String>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);

        loop {
            if *shutdown_rx.borrow() {
                break;
            }

            let parts: Vec<&str> = cmd.split_whitespace().collect();
            let Some((program, args)) = parts.split_first() else {
                eprintln!("[{name}] empty command");
                break;
            };

            let mut child = match Command::new(program)
                .args(args)
                .envs(&env)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
            {
                Ok(child) => child,
                Err(e) => {
                    eprintln!("[{name}] failed to start: {e}");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                    continue;
                }
            };

            if let Some(stdout) = child.stdout.take() {
                let prefix = name.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stdout).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        println!("[{prefix}] {line}");
                    }
                });
            }

            if let Some(stderr) = child.stderr.take() {
                let prefix = name.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        eprintln!("[{prefix}] {line}");
                    }
                });
            }

            tokio::select! {
                status = child.wait() => {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                    match status {
                        Ok(s) => eprintln!("[{name}] exited with {s}, restarting in {backoff:?}"),
                        Err(e) => eprintln!("[{name}] error: {e}, restarting in {backoff:?}"),
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
                _ = shutdown_rx.changed() => {
                    let _ = child.kill().await;
                    break;
                }
            }
        }
    })
}

async fn start_container(
    runtime: &str,
    name: &str,
    image: &str,
    service: &Service,
    app_name: &str,
    port: Option<u16>,
) -> Result<()> {
    let _ = Command::new(runtime)
        .args(["rm", "-f", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    let mut cmd = Command::new(runtime);
    cmd.args(["run", "-d", "--name", name]);

    if let Some(p) = port {
        cmd.arg("-p").arg(format!("{p}:{p}"));
    }

    if let Some(volume) = &service.volume {
        let mount_path = default_volume_path(image);
        cmd.arg("-v")
            .arg(format!("baton-{name}-{volume}:{mount_path}"));
    }

    for (key, val) in container_env_vars(image, app_name) {
        cmd.arg("-e").arg(format!("{key}={val}"));
    }

    cmd.arg(image);

    let output = cmd.output().await?;
    if !output.status.success() {
        bail!(
            "failed to start container {}: {}",
            name,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

async fn stop_container(runtime: &str, name: &str) {
    let _ = Command::new(runtime)
        .args(["stop", "-t", "5", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
    let _ = Command::new(runtime)
        .args(["rm", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

async fn detect_container_runtime() -> Result<String> {
    let docker = Command::new("docker")
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    if docker.map(|s| s.success()).unwrap_or(false) {
        return Ok("docker".to_string());
    }

    let podman = Command::new("podman")
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    if podman.map(|s| s.success()).unwrap_or(false) {
        return Ok("podman".to_string());
    }

    bail!("no container runtime found, install docker or podman")
}

pub fn toposort(services: &[Service]) -> Result<Vec<String>> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for service in services {
        in_degree.entry(&service.name).or_insert(0);
        for dep in &service.after {
            *in_degree.entry(&service.name).or_insert(0) += 1;
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(&service.name);
        }
    }

    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|&(_, &deg)| deg == 0)
        .map(|(&name, _)| name)
        .collect();

    queue.sort();

    let mut order = Vec::new();

    while let Some(name) = queue.pop() {
        order.push(name.to_string());
        if let Some(deps) = dependents.get(name) {
            for dep in deps {
                let deg = in_degree.get_mut(dep).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push(dep);
                }
            }
        }
    }

    if order.len() != services.len() {
        bail!("circular dependency detected in service definitions");
    }

    Ok(order)
}

pub fn register_env_vars(
    service: &Service,
    app_name: &str,
    port: u16,
    env: &mut HashMap<String, String>,
) {
    let key = service.name.to_uppercase();
    env.insert(format!("{key}_HOST"), "localhost".to_string());
    env.insert(format!("{key}_PORT"), port.to_string());

    if let Some(image) = &service.image {
        if image.contains("postgres") {
            env.insert(
                "DATABASE_URL".to_string(),
                format!("postgres://postgres:baton@localhost:{port}/{app_name}"),
            );
        } else if image.contains("redis") {
            env.insert("REDIS_URL".to_string(), format!("redis://localhost:{port}"));
        } else if image.contains("mysql") || image.contains("mariadb") {
            env.insert(
                "DATABASE_URL".to_string(),
                format!("mysql://root:baton@localhost:{port}/{app_name}"),
            );
        } else if image.contains("mongo") {
            env.insert(
                "MONGO_URL".to_string(),
                format!("mongodb://localhost:{port}/{app_name}"),
            );
        }
    }
}

pub fn default_port_for_image(image: &str) -> Option<u16> {
    if image.contains("postgres") {
        Some(5432)
    } else if image.contains("redis") {
        Some(6379)
    } else if image.contains("mysql") || image.contains("mariadb") {
        Some(3306)
    } else if image.contains("mongo") {
        Some(27017)
    } else if image.contains("rabbitmq") {
        Some(5672)
    } else if image.contains("nats") {
        Some(4222)
    } else {
        None
    }
}

fn default_volume_path(image: &str) -> &str {
    if image.contains("postgres") {
        "/var/lib/postgresql/data"
    } else if image.contains("redis") {
        "/data"
    } else if image.contains("mysql") || image.contains("mariadb") {
        "/var/lib/mysql"
    } else if image.contains("mongo") {
        "/data/db"
    } else {
        "/data"
    }
}

fn container_env_vars(image: &str, app_name: &str) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    if image.contains("postgres") {
        vars.push(("POSTGRES_PASSWORD".to_string(), "baton".to_string()));
        vars.push(("POSTGRES_DB".to_string(), app_name.to_string()));
    } else if image.contains("mysql") || image.contains("mariadb") {
        vars.push(("MYSQL_ROOT_PASSWORD".to_string(), "baton".to_string()));
        vars.push(("MYSQL_DATABASE".to_string(), app_name.to_string()));
    }
    vars
}
