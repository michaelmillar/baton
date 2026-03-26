use std::collections::HashMap;
use std::process::Stdio;
use std::str::FromStr;

use tokio::process::Command;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::config::normalise_cron;

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
                .unwrap_or(std::time::Duration::from_secs(1));

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

            let parts: Vec<&str> = cmd.split_whitespace().collect();
            let Some((program, args)) = parts.split_first() else {
                eprintln!("[{name}] empty command");
                break;
            };

            let child = Command::new(program)
                .args(args)
                .envs(&env)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .kill_on_drop(true)
                .spawn();

            match child {
                Ok(mut c) => {
                    tokio::select! {
                        status = c.wait() => {
                            match status {
                                Ok(s) => println!("[{name}] finished with {s}"),
                                Err(e) => eprintln!("[{name}] error: {e}"),
                            }
                        }
                        _ = shutdown_rx.changed() => {
                            let _ = c.kill().await;
                            break;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[{name}] failed to start: {e}");
                }
            }
        }
    })
}
