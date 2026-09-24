mod auth;
mod chat;
mod http;

use auth::Keys;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use ferrochat_core::Config;
use ferrochat_db::Db;
use jsonwebtoken::{DecodingKey, EncodingKey};
use rust_embed::RustEmbed;
use socketioxide::extract::SocketRef;
use socketioxide::SocketIo;
use std::collections::HashMap;
use std::sync::{atomic::AtomicUsize, Arc, Mutex};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

pub struct App {
    pub db: Db,
    pub keys: Keys,
    pub sockets: Mutex<HashMap<String, SocketRef>>,
    pub tasks: Mutex<HashMap<String, CancellationToken>>,
    pub chat_tasks: Mutex<HashMap<String, Vec<String>>>,
    pub key_tick: AtomicUsize,
    pub data_dir: std::path::PathBuf,
    pub frontend_dir: Option<std::path::PathBuf>,
}

#[derive(RustEmbed)]
#[folder = "../../../frontend/build"]
struct Assets;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    if std::env::args().any(|arg| arg == "healthcheck") {
        let port = std::env::var("FERROCHAT_PORT").unwrap_or_else(|_| "8080".into());
        let url = format!("http://127.0.0.1:{port}/health");
        let ok = reqwest::Client::new()
            .get(url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        std::process::exit(if ok { 0 } else { 1 });
    }

    let config = Config::from_env();
    tokio::fs::create_dir_all(&config.data_dir).await?;
    let secret = load_secret(&config).await?;
    let db = Db::connect(&config.database_url())
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let (layer, io) = SocketIo::builder().req_path("/ws/socket.io").build_layer();
    let app_state = Arc::new(App {
        db,
        keys: Keys {
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
        },
        sockets: Mutex::new(HashMap::new()),
        tasks: Mutex::new(HashMap::new()),
        chat_tasks: Mutex::new(HashMap::new()),
        key_tick: AtomicUsize::new(0),
        data_dir: config.data_dir.clone(),
        frontend_dir: config.frontend_dir.clone(),
    });
    let sockets = app_state.clone();
    io.ns("/", move |socket: SocketRef| {
        let id = socket.id.to_string();
        sockets.sockets.lock().unwrap().insert(id, socket.clone());
        let _ = socket.emit("user-list", &serde_json::json!({"user_ids": []}));
        let _ = socket.emit("usage", &serde_json::json!({"models": {}}));
    });

    let state = app_state.clone();
    let router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(http::router())
        .fallback(spa)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(layer)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.host, config.port);
    tracing::info!("Ferrochat listening on {addr}");
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

async fn load_secret(config: &Config) -> anyhow::Result<String> {
    if let Some(key) = &config.secret_key {
        return Ok(key.clone());
    }
    let path = config.data_dir.join("secret.key");
    if let Ok(existing) = tokio::fs::read_to_string(&path).await {
        if !existing.trim().is_empty() {
            return Ok(existing.trim().to_string());
        }
    }
    let generated = uuid::Uuid::new_v4().to_string();
    tokio::fs::write(&path, &generated).await?;
    Ok(generated)
}

async fn spa(State(app): State<Arc<App>>, req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');
    if path.starts_with("api/") || path.starts_with("ollama/") || path.starts_with("openai/") {
        tracing::warn!("no route for /{path}");
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"detail":"not found"}"#))
            .unwrap();
    }
    let path = if path.is_empty() { "index.html" } else { path };
    if let Some(dir) = &app.frontend_dir {
        let file = dir.join(path);
        let file = if file.is_file() {
            file
        } else {
            dir.join("index.html")
        };
        if let Ok(bytes) = tokio::fs::read(&file).await {
            let mime = mime_guess::from_path(&file).first_or_octet_stream();
            return Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(bytes))
                .unwrap();
        }
    }
    let asset_path = if Assets::get(path).is_some() {
        path
    } else {
        "index.html"
    };
    match Assets::get(asset_path) {
        Some(file) => {
            let mime = mime_guess::from_path(asset_path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(file.data.into_owned()))
                .unwrap()
        }
        None => (StatusCode::NOT_FOUND, "frontend is not built").into_response(),
    }
}
