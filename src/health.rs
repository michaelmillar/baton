use std::time::Duration;

use anyhow::{bail, Result};
use tokio::net::TcpStream;

pub async fn wait_for_port(port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    for _ in 0..30 {
        if TcpStream::connect(&addr).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    bail!("port {port} did not become available within 15s")
}

pub async fn wait_for_healthy(port: u16, path: &str) -> Result<()> {
    wait_for_port(port).await?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let url = format!("http://127.0.0.1:{port}{path}");

    for attempt in 0..20 {
        match client.get(&url).send().await {
            Ok(resp) if resp.status().as_u16() >= 200 && resp.status().as_u16() < 400 => {
                return Ok(());
            }
            Ok(resp) => {
                if attempt >= 19 {
                    bail!("health check {path} on :{port} returned {}", resp.status());
                }
            }
            Err(_) => {
                if attempt >= 19 {
                    bail!("health check {path} on :{port} failed to connect");
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    bail!("health check {path} on :{port} did not pass within 10s")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn port_not_listening_fails() {
        let result = wait_for_port(59999).await;
        assert!(result.is_err());
    }
}
