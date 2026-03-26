use std::time::Duration;

use anyhow::{bail, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

    let addr = format!("127.0.0.1:{port}");

    for attempt in 0..20 {
        match check_http(&addr, path).await {
            Ok(status) if status >= 200 && status < 400 => return Ok(()),
            Ok(status) => {
                if attempt < 19 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                } else {
                    bail!("health check {path} on :{port} returned {status}");
                }
            }
            Err(_) => {
                if attempt < 19 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                } else {
                    bail!("health check {path} on :{port} failed to connect");
                }
            }
        }
    }

    bail!("health check {path} on :{port} did not pass within 10s")
}

async fn check_http(addr: &str, path: &str) -> Result<u16> {
    let mut stream = TcpStream::connect(addr).await?;

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await?;

    let mut buf = vec![0u8; 1024];
    let n = stream.read(&mut buf).await?;
    let response = String::from_utf8_lossy(&buf[..n]);

    let status_line = response
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty response"))?;

    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("malformed status line"))?
        .parse()?;

    Ok(status_code)
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
