use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode, Uri};
use axum::Router;
use tokio::sync::watch;
use tokio::task::JoinHandle;

struct ProxyState {
    routes: HashMap<String, SocketAddr>,
    default_backend: Option<SocketAddr>,
}

pub struct ProxyRoute {
    pub domain: String,
    pub backend: SocketAddr,
}

pub fn spawn_proxy(
    routes: Vec<ProxyRoute>,
    listen_port: u16,
    mut shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut route_map = HashMap::new();
        let mut default_backend = None;

        for (i, route) in routes.iter().enumerate() {
            println!("  [proxy] {} -> {}", route.domain, route.backend);
            route_map.insert(route.domain.clone(), route.backend);
            if i == 0 {
                default_backend = Some(route.backend);
            }
        }

        let state = Arc::new(ProxyState {
            routes: route_map,
            default_backend,
        });

        let app = Router::new()
            .fallback(proxy_handler)
            .with_state(state);

        let addr = SocketAddr::from(([0, 0, 0, 0], listen_port));
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[proxy] failed to bind :{listen_port}: {e}");
                return;
            }
        };

        println!("  [proxy] listening on :{listen_port}");

        let server = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.changed().await;
            });

        if let Err(e) = server.await {
            eprintln!("[proxy] server error: {e}");
        }
    })
}

async fn proxy_handler(
    State(state): State<Arc<ProxyState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let host = req
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");

    let backend = state
        .routes
        .get(host)
        .or(state.default_backend.as_ref())
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let path = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    let uri: Uri = format!("http://{backend}{path}")
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    let mut proxy_req = Request::builder()
        .method(req.method())
        .uri(uri);

    for (key, value) in req.headers() {
        if key != "host" {
            proxy_req = proxy_req.header(key, value);
        }
    }

    let proxy_req = proxy_req
        .body(req.into_body())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let resp = client
        .request(proxy_req)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let (parts, incoming) = resp.into_parts();
    let body = Body::new(incoming);
    Ok(Response::from_parts(parts, body))
}
