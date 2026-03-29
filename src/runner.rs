use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::build;
use crate::config::{Config, Service};
use crate::cron::spawn_cron_task;
use crate::dashboard::{self, ServiceState, SharedState};
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

pub async fn run(config: Config, ui_port: Option<u16>) -> Result<()> {
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
    let shared_state = dashboard::new_shared_state();

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

            start_container(rt, &container_name, &image_tag, &build_svc, &config.app.name, port, &env_vars).await?;

            if let Some(p) = port {
                if let Some(health_path) = &service.health {
                    health::wait_for_healthy(p, health_path).await?;
                } else {
                    health::wait_for_port(p).await?;
                }
                register_env_vars(service, &config.app.name, p, &mut env_vars);
                collect_proxy_route(&config, service, p, &mut proxy_routes);
            }

            set_state(&shared_state, &service.name, "container", &image_tag, port, None, "running").await;
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

            start_container(rt, &container_name, image, service, &config.app.name, port, &env_vars).await?;

            if let Some(p) = port {
                health::wait_for_port(p).await?;
                register_env_vars(service, &config.app.name, p, &mut env_vars);
            }

            set_state(&shared_state, &service.name, "container", image, port, None, "running").await;
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

                set_state(&shared_state, &service.name, "cron", cmd, None, Some(schedule.clone()), "scheduled").await;
                println!("  [ok] {}  {} scheduled ({})", service.name, cmd, schedule);
                handles.push((service.name.clone(), ServiceHandle::Cron { task }));
            } else {
                let task = spawn_process(
                    service.name.clone(),
                    cmd.clone(),
                    env_vars.clone(),
                    shutdown_rx.clone(),
                    shared_state.clone(),
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

                set_state(&shared_state, &service.name, "process", cmd, service.port, None, "running").await;
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
            set_state(&shared_state, &service.name, mode, dir, Some(port), None, "running").await;
            println!("  [ok] {}  {} ({}) on :{}", service.name, dir, mode, port);
            handles.push((service.name.clone(), ServiceHandle::Static { task }));
        }
    }

    if let Some(port) = ui_port {
        let task = dashboard::spawn_dashboard(
            config.app.domain.clone(),
            shared_state.clone(),
            port,
            shutdown_rx.clone(),
        );
        handles.push(("ui".to_string(), ServiceHandle::Proxy { task }));
    }

    if !proxy_routes.is_empty() {
        let proxy_port = config.app.proxy_port.unwrap_or(8443);
        let task = proxy::spawn_proxy(proxy_routes, proxy_port, shutdown_rx.clone());
        handles.push(("proxy".to_string(), ServiceHandle::Proxy { task }));
    }

    println!("\nall services running. ctrl+c to stop.\n");

    tokio::signal::ctrl_c().await?;
    println!("\nshutting down...");

    let _ = shutdown_tx.send(true);

    for (name, handle) in handles.iter().rev() {
        if let ServiceHandle::Container { runtime, container_name } = handle {
            stop_container(runtime, container_name).await;
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

async fn set_state(
    shared: &SharedState,
    name: &str,
    kind: &str,
    detail: &str,
    port: Option<u16>,
    schedule: Option<String>,
    status: &str,
) {
    let mut state = shared.write().await;
    let entry = state.entry(name.to_string()).or_insert_with(|| ServiceState {
        name: name.to_string(),
        kind: kind.to_string(),
        detail: detail.to_string(),
        port,
        schedule,
        status: status.to_string(),
        restarts: 0,
    });
    entry.status = status.to_string();
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

const HEALTHY_THRESHOLD: Duration = Duration::from_secs(60);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

fn spawn_process(
    name: String,
    cmd: String,
    env: HashMap<String, String>,
    mut shutdown_rx: watch::Receiver<bool>,
    shared_state: SharedState,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        let mut restarts: u32 = 0;

        loop {
            if *shutdown_rx.borrow() {
                break;
            }

            let mut child = match Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .envs(&env)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(e) => {
                    eprintln!("[{name}] failed to start: {e}");
                    update_status(&shared_state, &name, "crashed", restarts).await;
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                    continue;
                }
            };

            update_status(&shared_state, &name, "running", restarts).await;
            let started = Instant::now();

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
                    restarts += 1;
                    if started.elapsed() > HEALTHY_THRESHOLD {
                        backoff = Duration::from_secs(1);
                    }
                    update_status(&shared_state, &name, "restarting", restarts).await;
                    match status {
                        Ok(s) => eprintln!("[{name}] exited with {s}, restarting in {backoff:?}"),
                        Err(e) => eprintln!("[{name}] error: {e}, restarting in {backoff:?}"),
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
                _ = shutdown_rx.changed() => {
                    graceful_kill(&mut child, &name).await;
                    update_status(&shared_state, &name, "stopped", restarts).await;
                    break;
                }
            }
        }
    })
}

async fn update_status(shared: &SharedState, name: &str, status: &str, restarts: u32) {
    let mut state = shared.write().await;
    if let Some(entry) = state.get_mut(name) {
        entry.status = status.to_string();
        entry.restarts = restarts;
    }
}

async fn graceful_kill(child: &mut tokio::process::Child, name: &str) {
    if let Some(pid) = child.id() {
        unsafe { libc::kill(pid as i32, libc::SIGTERM); }
        tokio::select! {
            _ = child.wait() => (),
            _ = tokio::time::sleep(SHUTDOWN_GRACE) => {
                eprintln!("[{name}] did not stop within {}s, sending SIGKILL",
                    SHUTDOWN_GRACE.as_secs());
                let _ = child.kill().await;
            }
        }
    } else {
        let _ = child.kill().await;
    }
}

async fn start_container(
    runtime: &str,
    name: &str,
    image: &str,
    service: &Service,
    app_name: &str,
    port: Option<u16>,
    user_env: &HashMap<String, String>,
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

    for (key, val) in container_env_vars(image, app_name, user_env) {
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
            let pw = env.get("POSTGRES_PASSWORD")
                .cloned()
                .unwrap_or_else(|| generate_password(app_name, "postgres"));
            env.insert(
                "DATABASE_URL".to_string(),
                format!("postgres://postgres:{pw}@localhost:{port}/{app_name}"),
            );
        } else if image.contains("redis") {
            env.insert("REDIS_URL".to_string(), format!("redis://localhost:{port}"));
        } else if image.contains("mysql") || image.contains("mariadb") {
            let pw = env.get("MYSQL_ROOT_PASSWORD")
                .cloned()
                .unwrap_or_else(|| generate_password(app_name, "mysql"));
            env.insert(
                "DATABASE_URL".to_string(),
                format!("mysql://root:{pw}@localhost:{port}/{app_name}"),
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

fn container_env_vars(image: &str, app_name: &str, user_env: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    if image.contains("postgres") {
        let pw = user_env.get("POSTGRES_PASSWORD")
            .cloned()
            .unwrap_or_else(|| generate_password(app_name, "postgres"));
        vars.push(("POSTGRES_PASSWORD".to_string(), pw));
        vars.push(("POSTGRES_DB".to_string(), app_name.to_string()));
    } else if image.contains("mysql") || image.contains("mariadb") {
        let pw = user_env.get("MYSQL_ROOT_PASSWORD")
            .cloned()
            .unwrap_or_else(|| generate_password(app_name, "mysql"));
        vars.push(("MYSQL_ROOT_PASSWORD".to_string(), pw));
        vars.push(("MYSQL_DATABASE".to_string(), app_name.to_string()));
    }
    vars
}

fn generate_password(app_name: &str, db_type: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    app_name.hash(&mut hasher);
    db_type.hash(&mut hasher);
    let hash = hasher.finish();
    format!("baton_{hash:016x}")
}
