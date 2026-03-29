use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use chrono::Utc;
use tokio::process::Command;

use crate::config::Service;

const BATON_DIR: &str = ".baton";
const SNAPSHOTS_DIR: &str = "snapshots";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotMeta {
    pub id: String,
    pub timestamp: String,
    pub services: Vec<ServiceSnapshot>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServiceSnapshot {
    pub name: String,
    pub method: String,
    pub file: String,
}

pub fn snapshot_dir() -> PathBuf {
    PathBuf::from(BATON_DIR).join(SNAPSHOTS_DIR)
}

fn snapshot_path(id: &str) -> PathBuf {
    snapshot_dir().join(id)
}

pub async fn take_snapshot(
    services: &[Service],
    app_name: &str,
    env_vars: &HashMap<String, String>,
) -> Result<SnapshotMeta> {
    let id = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let dir = snapshot_path(&id);
    std::fs::create_dir_all(&dir)?;

    let mut snapshots = Vec::new();

    for service in services {
        if let Some(snap) = snapshot_service(service, app_name, &dir, env_vars).await? {
            snapshots.push(snap);
        }
    }

    if snapshots.is_empty() {
        std::fs::remove_dir_all(&dir)?;
        bail!("no services with backup configuration found");
    }

    let meta = SnapshotMeta {
        id: id.clone(),
        timestamp: Utc::now().to_rfc3339(),
        services: snapshots,
    };

    let meta_path = dir.join("meta.json");
    std::fs::write(&meta_path, serde_json::to_string_pretty(&meta)?)?;

    Ok(meta)
}

async fn snapshot_service(
    service: &Service,
    app_name: &str,
    dir: &Path,
    env_vars: &HashMap<String, String>,
) -> Result<Option<ServiceSnapshot>> {
    let method = match resolve_backup_method(service) {
        Some(m) => m,
        None => return Ok(None),
    };

    match method.as_str() {
        "pg_dump" => snapshot_postgres(service, app_name, dir, env_vars).await,
        "redis" => snapshot_redis(service, app_name, dir).await,
        custom => snapshot_custom(service, custom, dir, env_vars).await,
    }
}

pub fn resolve_has_backup(service: &Service) -> bool {
    resolve_backup_method(service).is_some()
}

fn resolve_backup_method(service: &Service) -> Option<String> {
    if let Some(ref backup) = service.backup {
        return Some(backup.clone());
    }

    if let Some(ref image) = service.image {
        if image.contains("postgres") {
            return Some("pg_dump".to_string());
        }
        if image.contains("redis") {
            return Some("redis".to_string());
        }
    }

    None
}

async fn snapshot_postgres(
    service: &Service,
    app_name: &str,
    dir: &Path,
    env_vars: &HashMap<String, String>,
) -> Result<Option<ServiceSnapshot>> {
    let container_name = format!("baton-{}-{}", app_name, service.name);
    let file = format!("{}.sql.gz", service.name);
    let file_path = dir.join(&file);

    let dump = Command::new("docker")
        .args([
            "exec", &container_name,
            "pg_dump", "-U", "postgres", "--clean", "--if-exists", app_name,
        ])
        .envs(env_vars)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !dump.status.success() {
        let stderr = String::from_utf8_lossy(&dump.stderr);
        bail!("pg_dump failed for {}: {}", service.name, stderr.trim());
    }

    let compressed = compress_bytes(&dump.stdout)?;
    std::fs::write(&file_path, compressed)?;

    Ok(Some(ServiceSnapshot {
        name: service.name.clone(),
        method: "pg_dump".to_string(),
        file,
    }))
}

async fn snapshot_redis(
    service: &Service,
    app_name: &str,
    dir: &Path,
) -> Result<Option<ServiceSnapshot>> {
    let container_name = format!("baton-{}-{}", app_name, service.name);

    let bgsave = Command::new("docker")
        .args(["exec", &container_name, "redis-cli", "BGSAVE"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !bgsave.status.success() {
        let stderr = String::from_utf8_lossy(&bgsave.stderr);
        bail!("redis BGSAVE failed for {}: {}", service.name, stderr.trim());
    }

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let file = format!("{}.rdb", service.name);
    let file_path = dir.join(&file);

    let copy = Command::new("docker")
        .args([
            "cp",
            &format!("{container_name}:/data/dump.rdb"),
            &file_path.to_string_lossy().to_string(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !copy.status.success() {
        let stderr = String::from_utf8_lossy(&copy.stderr);
        bail!("redis snapshot copy failed for {}: {}", service.name, stderr.trim());
    }

    Ok(Some(ServiceSnapshot {
        name: service.name.clone(),
        method: "redis".to_string(),
        file,
    }))
}

async fn snapshot_custom(
    service: &Service,
    command: &str,
    dir: &Path,
    env_vars: &HashMap<String, String>,
) -> Result<Option<ServiceSnapshot>> {
    let file = format!("{}.backup", service.name);
    let file_path = dir.join(&file);

    let mut extended_env = env_vars.clone();
    extended_env.insert("BATON_SNAPSHOT_PATH".to_string(), file_path.to_string_lossy().to_string());
    extended_env.insert("BATON_SERVICE_NAME".to_string(), service.name.clone());

    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .envs(&extended_env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("backup command failed for {}: {}", service.name, stderr.trim());
    }

    if !file_path.exists() {
        bail!(
            "backup command for {} did not produce a file at {}",
            service.name,
            file_path.display()
        );
    }

    Ok(Some(ServiceSnapshot {
        name: service.name.clone(),
        method: command.to_string(),
        file,
    }))
}

pub async fn restore_snapshot(
    meta: &SnapshotMeta,
    services: &[Service],
    app_name: &str,
    env_vars: &HashMap<String, String>,
) -> Result<()> {
    let dir = snapshot_path(&meta.id);

    for snap in &meta.services {
        let service = services
            .iter()
            .find(|s| s.name == snap.name)
            .ok_or_else(|| anyhow::anyhow!("service '{}' not found in config", snap.name))?;

        let file_path = dir.join(&snap.file);

        match snap.method.as_str() {
            "pg_dump" => restore_postgres(service, app_name, &file_path, env_vars).await?,
            "redis" => restore_redis(service, app_name, &file_path).await?,
            custom => restore_custom(service, custom, &file_path, env_vars).await?,
        }
    }

    Ok(())
}

async fn restore_postgres(
    service: &Service,
    app_name: &str,
    file_path: &Path,
    env_vars: &HashMap<String, String>,
) -> Result<()> {
    let container_name = format!("baton-{}-{}", app_name, service.name);

    let compressed = std::fs::read(file_path)?;
    let sql = decompress_bytes(&compressed)?;

    let mut child = Command::new("docker")
        .args([
            "exec", "-i", &container_name,
            "psql", "-U", "postgres", "-d", app_name,
        ])
        .envs(env_vars)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(&sql).await?;
        stdin.shutdown().await?;
    }

    let output = child.wait_with_output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("pg restore failed for {}: {}", service.name, stderr.trim());
    }

    Ok(())
}

async fn restore_redis(
    service: &Service,
    app_name: &str,
    file_path: &Path,
) -> Result<()> {
    let container_name = format!("baton-{}-{}", app_name, service.name);

    let copy = Command::new("docker")
        .args([
            "cp",
            &file_path.to_string_lossy().to_string(),
            &format!("{container_name}:/data/dump.rdb"),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !copy.status.success() {
        let stderr = String::from_utf8_lossy(&copy.stderr);
        bail!("redis restore copy failed for {}: {}", service.name, stderr.trim());
    }

    let restart = Command::new("docker")
        .args(["restart", &container_name])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !restart.status.success() {
        let stderr = String::from_utf8_lossy(&restart.stderr);
        bail!("redis restart failed for {}: {}", service.name, stderr.trim());
    }

    Ok(())
}

async fn restore_custom(
    service: &Service,
    command: &str,
    file_path: &Path,
    env_vars: &HashMap<String, String>,
) -> Result<()> {
    let restore_cmd = format!("{command} --restore");

    let mut extended_env = env_vars.clone();
    extended_env.insert("BATON_SNAPSHOT_PATH".to_string(), file_path.to_string_lossy().to_string());
    extended_env.insert("BATON_SERVICE_NAME".to_string(), service.name.clone());
    extended_env.insert("BATON_RESTORE".to_string(), "1".to_string());

    let output = Command::new("sh")
        .arg("-c")
        .arg(&restore_cmd)
        .envs(&extended_env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("restore command failed for {}: {}", service.name, stderr.trim());
    }

    Ok(())
}

pub fn list_snapshots() -> Result<Vec<SnapshotMeta>> {
    let dir = snapshot_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut snapshots = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let meta_path = entry.path().join("meta.json");
        if meta_path.exists() {
            let content = std::fs::read_to_string(&meta_path)?;
            let meta: SnapshotMeta = serde_json::from_str(&content)?;
            snapshots.push(meta);
        }
    }

    Ok(snapshots)
}

pub fn load_snapshot(id: &str) -> Result<SnapshotMeta> {
    let meta_path = snapshot_path(id).join("meta.json");
    if !meta_path.exists() {
        bail!("snapshot '{}' not found", id);
    }
    let content = std::fs::read_to_string(&meta_path)?;
    let meta: SnapshotMeta = serde_json::from_str(&content)?;
    Ok(meta)
}

pub fn latest_snapshot() -> Result<Option<SnapshotMeta>> {
    let snapshots = list_snapshots()?;
    Ok(snapshots.into_iter().last())
}

fn compress_bytes(data: &[u8]) -> Result<Vec<u8>> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

fn decompress_bytes(data: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut decoder = flate2::read::GzDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}
