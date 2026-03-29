use std::collections::HashMap;
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

const DASHBOARD_HTML: &str = include_str!("dashboard.html");

#[derive(Debug, Clone, Serialize)]
pub struct ServiceState {
    pub name: String,
    pub kind: String,
    pub detail: String,
    pub port: Option<u16>,
    pub schedule: Option<String>,
    pub status: String,
    pub restarts: u32,
}

pub type SharedState = Arc<RwLock<HashMap<String, ServiceState>>>;

pub fn new_shared_state() -> SharedState {
    Arc::new(RwLock::new(HashMap::new()))
}

#[derive(Debug, Clone, Serialize)]
struct StatusResponse {
    domain: Option<String>,
    services: Vec<ServiceState>,
}

pub fn spawn_dashboard(
    domain: Option<String>,
    state: SharedState,
    port: u16,
    mut shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let app_state = Arc::new(DashState { domain, services: state });

        let app = Router::new()
            .route("/", get(dashboard_page))
            .route("/api/status", get(status_api))
            .with_state(app_state);

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

struct DashState {
    domain: Option<String>,
    services: SharedState,
}

async fn dashboard_page() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], DASHBOARD_HTML)
}

async fn status_api(
    State(state): State<Arc<DashState>>,
) -> Json<StatusResponse> {
    let services = state.services.read().await;
    let mut list: Vec<ServiceState> = services.values().cloned().collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Json(StatusResponse {
        domain: state.domain.clone(),
        services: list,
    })
}
