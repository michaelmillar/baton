use std::collections::HashMap;
use std::process::Stdio;
use std::str::FromStr;
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::config::normalise_cron;

const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

pub fn spawn_cron_task(
    name: String,
    cmd: String,
    schedule_expr: String,
    env: HashMap<String, String>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let expr = normalise_cron(&schedule_expr);
        let schedule = match cron::Schedule::from_str(&expr) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[{name}] invalid schedule '{schedule_expr}': {e}");
                return;
            }
        };

        loop {
            let next = match schedule.upcoming(chrono::Utc).next() {
                Some(t) => t,
                None => {
                    eprintln!("[{name}] no upcoming schedule times");
                    break;
                }
            };

            let wait = (next - chrono::Utc::now())
                .to_std()
                .unwrap_or(Duration::from_secs(1));

            println!("[{name}] next run at {next}");

            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = shutdown_rx.changed() => {
                    break;
                }
            }

            if *shutdown_rx.borrow() {
                break;
            }

            println!("[{name}] running...");

            let mut child = match Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .envs(&env)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[{name}] failed to start: {e}");
                    continue;
                }
            };

            tokio::select! {
                status = child.wait() => {
                    match status {
                        Ok(s) => println!("[{name}] finished with {s}"),
                        Err(e) => eprintln!("[{name}] error: {e}"),
                    }
                }
                _ = shutdown_rx.changed() => {
                    graceful_kill(&mut child, &name).await;
                    break;
                }
            }
        }
    })
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
