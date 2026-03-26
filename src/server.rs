use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub address: String,
    pub hostname: String,
    pub capacity: NodeCapacity,
    pub services: Vec<String>,
    pub last_heartbeat: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapacity {
    pub cpus: u32,
    pub memory_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAssignment {
    pub service_name: String,
    pub agent_address: String,
    pub status: ServiceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceStatus {
    Pending,
    Starting,
    Running,
    Failed,
    Stopped,
}

const DASHBOARD_HTML: &str = include_str!("dashboard.html");

#[derive(Debug, Serialize)]
struct ClusterStatus {
    app: String,
    domain: Option<String>,
    services: Vec<ClusterServiceInfo>,
    agents: Vec<AgentInfo>,
    assignments: Vec<ServiceAssignment>,
}

#[derive(Debug, Serialize)]
struct ClusterServiceInfo {
    name: String,
    run: Option<String>,
    image: Option<String>,
    build: Option<String>,
    #[serde(rename = "static")]
    static_dir: Option<String>,
    port: Option<u16>,
    schedule: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    address: String,
    hostname: String,
    capacity: NodeCapacity,
}

#[derive(Debug, Deserialize)]
struct HeartbeatRequest {
    address: String,
    services: Vec<RunningServiceReport>,
}

#[derive(Debug, Deserialize)]
struct RunningServiceReport {
    name: String,
    status: ServiceStatus,
}

struct ServerState {
    config: Config,
    agents: RwLock<HashMap<String, AgentInfo>>,
    assignments: RwLock<Vec<ServiceAssignment>>,
}

pub async fn run(config: Config, port: u16) -> Result<()> {
    let state = Arc::new(ServerState {
        config,
        agents: RwLock::new(HashMap::new()),
        assignments: RwLock::new(Vec::new()),
    });

    let app = Router::new()
        .route("/", get(dashboard_page))
        .route("/api/status", get(status_handler))
        .route("/api/agents/register", post(register_handler))
        .route("/api/agents/heartbeat", post(heartbeat_handler))
        .route("/api/deploy", post(deploy_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("baton server listening on :{port}");
    println!("  dashboard: http://localhost:{port}");
    println!("  waiting for agents to register...\n");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    Ok(())
}

async fn dashboard_page() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], DASHBOARD_HTML)
}

async fn status_handler(
    State(state): State<Arc<ServerState>>,
) -> Json<ClusterStatus> {
    let agents = state.agents.read().await;
    let assignments = state.assignments.read().await;

    let services = state.config.services.iter().map(|s| ClusterServiceInfo {
        name: s.name.clone(),
        run: s.run.clone(),
        image: s.image.clone(),
        build: s.build.clone(),
        static_dir: s.static_dir.clone(),
        port: s.port,
        schedule: s.schedule.clone(),
    }).collect();

    Json(ClusterStatus {
        app: state.config.app.name.clone(),
        domain: state.config.app.domain.clone(),
        services,
        agents: agents.values().cloned().collect(),
        assignments: assignments.clone(),
    })
}

async fn register_handler(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut agents = state.agents.write().await;

    let info = AgentInfo {
        address: req.address.clone(),
        hostname: req.hostname,
        capacity: req.capacity,
        services: vec![],
        last_heartbeat: chrono::Utc::now().timestamp(),
    };

    println!("[server] agent registered: {}", req.address);
    agents.insert(req.address.clone(), info);

    let agent_count = agents.len();
    drop(agents);

    if agent_count > 0 {
        schedule_services(&state).await;
    }

    Ok(Json(serde_json::json!({ "status": "registered" })))
}

async fn heartbeat_handler(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<HeartbeatRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut agents = state.agents.write().await;

    if let Some(agent) = agents.get_mut(&req.address) {
        agent.last_heartbeat = chrono::Utc::now().timestamp();
        agent.services = req.services.iter().map(|s| s.name.clone()).collect();
    }

    let mut assignments = state.assignments.write().await;
    for report in &req.services {
        if let Some(assignment) = assignments.iter_mut().find(|a| a.service_name == report.name && a.agent_address == req.address) {
            assignment.status = report.status.clone();
        }
    }

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

async fn deploy_handler(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    schedule_services(&state).await;
    let assignments = state.assignments.read().await;
    Ok(Json(serde_json::json!({
        "status": "deployed",
        "assignments": assignments.len()
    })))
}

async fn schedule_services(state: &ServerState) {
    let agents = state.agents.read().await;
    let agent_list: Vec<&AgentInfo> = agents.values().collect();

    if agent_list.is_empty() {
        return;
    }

    let mut assignments = state.assignments.write().await;
    assignments.clear();

    for (i, service) in state.config.services.iter().enumerate() {
        let agent = &agent_list[i % agent_list.len()];
        assignments.push(ServiceAssignment {
            service_name: service.name.clone(),
            agent_address: agent.address.clone(),
            status: ServiceStatus::Pending,
        });
        println!("[server] assigned '{}' -> {}", service.name, agent.address);
    }
}
