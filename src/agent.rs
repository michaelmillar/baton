use std::time::Duration;

use anyhow::Result;
use serde::Serialize;
use tokio::sync::watch;

use crate::server::{NodeCapacity, ServiceStatus};

#[derive(Debug, Serialize)]
struct RegisterPayload {
    address: String,
    hostname: String,
    capacity: NodeCapacity,
}

#[derive(Debug, Serialize)]
struct HeartbeatPayload {
    address: String,
    services: Vec<ServiceReport>,
}

#[derive(Debug, Serialize)]
struct ServiceReport {
    name: String,
    status: ServiceStatus,
}

pub async fn run(server_addr: String, agent_port: u16) -> Result<()> {
    let hostname = gethostname();
    let agent_addr = format!("{}:{}", local_ip(), agent_port);

    println!("baton agent starting");
    println!("  server: {server_addr}");
    println!("  agent: {agent_addr}");

    register(&server_addr, &agent_addr, &hostname).await?;
    println!("  registered with server\n");

    let (_shutdown_tx, shutdown_rx) = watch::channel(false);

    let heartbeat_server = server_addr.clone();
    let heartbeat_addr = agent_addr.clone();
    let heartbeat_rx = shutdown_rx.clone();
    let heartbeat_handle = tokio::spawn(async move {
        heartbeat_loop(&heartbeat_server, &heartbeat_addr, heartbeat_rx).await;
    });

    println!("agent running. ctrl+c to stop.\n");
    tokio::signal::ctrl_c().await?;
    println!("\nshutting down agent...");

    heartbeat_handle.abort();

    println!("done.");
    Ok(())
}

async fn register(server: &str, agent_addr: &str, hostname: &str) -> Result<()> {
    let payload = RegisterPayload {
        address: agent_addr.to_string(),
        hostname: hostname.to_string(),
        capacity: get_capacity(),
    };

    let client = reqwest_client();
    let url = format!("http://{server}/api/agents/register");

    for attempt in 0..10 {
        match client.post(&url).json(&payload).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            Ok(resp) => {
                eprintln!("[agent] registration failed: {}", resp.status());
            }
            Err(e) => {
                if attempt < 9 {
                    eprintln!("[agent] server not reachable, retrying in 2s... ({e})");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                } else {
                    anyhow::bail!("failed to register with server after 10 attempts");
                }
            }
        }
    }

    Ok(())
}

async fn heartbeat_loop(
    server: &str,
    agent_addr: &str,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let client = reqwest_client();
    let url = format!("http://{server}/api/agents/heartbeat");

    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(10)) => {}
            _ = shutdown_rx.changed() => break,
        }

        if *shutdown_rx.borrow() {
            break;
        }

        let payload = HeartbeatPayload {
            address: agent_addr.to_string(),
            services: vec![],
        };

        let _ = client.post(&url).json(&payload).send().await;
    }
}

fn get_capacity() -> NodeCapacity {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);

    let memory_mb = sys_memory_mb().unwrap_or(1024);

    NodeCapacity { cpus, memory_mb }
}

fn sys_memory_mb() -> Option<u64> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = content.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

fn gethostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

fn reqwest_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default()
}
