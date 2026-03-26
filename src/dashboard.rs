use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tokio::sync::{watch, RwLock};
use tokio::task::JoinHandle;

use crate::config::Config;

const DASHBOARD_HTML: &str = include_str!("dashboard.html");

#[derive(Debug, Clone, Serialize)]
pub struct LocalStatus {
    pub domain: Option<String>,
    pub services: Vec<LocalServiceStatus>,
    pub agents: Vec<serde_json::Value>,
    pub assignments: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalServiceStatus {
    pub name: String,
    pub run: Option<String>,
    pub image: Option<String>,
    pub build: Option<String>,
    #[serde(rename = "static")]
    pub static_dir: Option<String>,
    pub port: Option<u16>,
    pub schedule: Option<String>,
    pub status: String,
}

pub fn build_local_status(config: &Config) -> LocalStatus {
    let services = config
        .services
        .iter()
        .map(|s| {
            let status = if s.schedule.is_some() {
                "scheduled"
            } else {
                "running"
            };
            LocalServiceStatus {
                name: s.name.clone(),
                run: s.run.clone(),
                image: s.image.clone(),
                build: s.build.clone(),
                static_dir: s.static_dir.clone(),
                port: s.port,
                schedule: s.schedule.clone(),
                status: status.to_string(),
            }
        })
        .collect();

    LocalStatus {
        domain: config.app.domain.clone(),
        services,
        agents: vec![],
        assignments: vec![],
    }
}

pub fn spawn_dashboard(
    config: Config,
    port: u16,
    mut shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let status = build_local_status(&config);
        let state = Arc::new(RwLock::new(status));

        let app = Router::new()
            .route("/", get(dashboard_page))
            .route("/api/status", get(status_api))
            .with_state(state);

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[ui] failed to bind :{port}: {e}");
                return;
            }
        };

        println!("  [ui] dashboard at http://localhost:{port}");

        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = shutdown_rx.changed().await;
        });

        if let Err(e) = server.await {
            eprintln!("[ui] error: {e}");
        }
    })
}

async fn dashboard_page() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], DASHBOARD_HTML)
}

async fn status_api(
    State(state): State<Arc<RwLock<LocalStatus>>>,
) -> Json<LocalStatus> {
    let status = state.read().await;
    Json(status.clone())
}
