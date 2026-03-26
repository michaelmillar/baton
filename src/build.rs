use std::process::Stdio;

use anyhow::{bail, Result};
use tokio::process::Command;

pub async fn build_image(
    runtime: &str,
    image_tag: &str,
    build_context: &str,
) -> Result<()> {
    println!("  [build] building {image_tag} from {build_context}...");

    let output = Command::new(runtime)
        .args(["build", "-t", image_tag, build_context])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .await?;

    if !output.status.success() {
        bail!("build failed for {image_tag}");
    }

    println!("  [build] {image_tag} done");
    Ok(())
}
