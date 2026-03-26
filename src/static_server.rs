use std::net::SocketAddr;
use std::path::PathBuf;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tower_http::services::{ServeDir, ServeFile};

pub fn spawn_static_server(
    name: String,
    dir: PathBuf,
    port: u16,
    spa: bool,
    mut shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let app = if spa {
            let fallback = ServeFile::new(dir.join("index.html"));
            axum::Router::new().fallback_service(ServeDir::new(&dir).fallback(fallback))
        } else {
            axum::Router::new().fallback_service(ServeDir::new(&dir))
        };

        let addr = SocketAddr::from(([0, 0, 0, 0], port));

        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[{name}] failed to bind :{port}: {e}");
                return;
            }
        };

        let server = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.changed().await;
            });

        if let Err(e) = server.await {
            eprintln!("[{name}] server error: {e}");
        }
    })
}
