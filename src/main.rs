mod commands;
mod crypto;
mod db;

use anyhow::{Context, Result};
use commands::RateLimiter;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

pub struct AppState {
    pub pool: sqlx::PgPool,
    pub http: reqwest::Client,
    pub mm_url: String,
    pub mm_token: String,
    pub bot_user_id: String,
    pub rate_limiter: RateLimiter,
    pub user_cache: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Deserialize)]
struct WsEvent {
    event: Option<String>,
    data: Option<WsEventData>,
}

#[derive(Deserialize)]
struct WsEventData {
    post: Option<String>,
    channel_type: Option<String>,
}

#[derive(Deserialize)]
struct MmPost {
    id: String,
    user_id: String,
    channel_id: String,
    message: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mm_url = std::env::var("MATTERMOST_URL")
        .context("MATTERMOST_URL environment variable is not set")?;
    let mm_url = mm_url.trim_end_matches('/').to_string();

    let mm_token = std::env::var("MATTERMOST_TOKEN")
        .context("MATTERMOST_TOKEN environment variable is not set")?;

    let db_url = std::env::var("DATABASE_URL")
        .context("DATABASE_URL environment variable is not set")?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .context("Failed to connect to PostgreSQL")?;
    info!("Connected to PostgreSQL");

    db::run_migrations(&pool)
        .await
        .context("Database migration failed")?;

    let http = reqwest::Client::builder()
        .build()
        .context("Failed to build HTTP client")?;

    let bot_user_id = fetch_bot_user_id(&http, &mm_url, &mm_token)
        .await
        .context("Failed to fetch bot user info from Mattermost")?;
    info!("Bot user ID: {}", bot_user_id);

    let state = Arc::new(AppState {
        pool,
        http,
        mm_url: mm_url.clone(),
        mm_token: mm_token.clone(),
        bot_user_id,
        rate_limiter: RateLimiter::new(),
        user_cache: Arc::new(Mutex::new(HashMap::new())),
    });

    let ws_url = mm_url
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let ws_url = format!("{}/api/v4/websocket", ws_url);

    run_websocket_loop(state, ws_url, mm_token).await
}

async fn fetch_bot_user_id(
    http: &reqwest::Client,
    mm_url: &str,
    token: &str,
) -> Result<String> {
    let resp: serde_json::Value = http
        .get(format!("{}/api/v4/users/me", mm_url))
        .bearer_auth(token)
        .send()
        .await
        .context("HTTP request to /api/v4/users/me failed")?
        .error_for_status()
        .context("Mattermost returned error for /api/v4/users/me")?
        .json()
        .await
        .context("Failed to parse /api/v4/users/me response")?;

    resp["id"]
        .as_str()
        .context("Missing 'id' field in /api/v4/users/me response")
        .map(str::to_string)
}

async fn run_websocket_loop(
    state: Arc<AppState>,
    ws_url: String,
    token: String,
) -> Result<()> {
    loop {
        match run_websocket(&state, &ws_url, &token).await {
            Ok(()) => warn!("WebSocket closed cleanly, reconnecting..."),
            Err(e) => {
                error!("WebSocket error: {:#}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
        info!("Reconnecting to WebSocket...");
    }
}

async fn run_websocket(
    state: &Arc<AppState>,
    ws_url: &str,
    token: &str,
) -> Result<()> {
    info!("Connecting to WebSocket: {}", ws_url);
    let (ws_stream, _) = connect_async(ws_url)
        .await
        .context("Failed to connect to Mattermost WebSocket")?;
    info!("WebSocket connected");

    let (mut write, mut read) = ws_stream.split();

    let auth_msg = json!({
        "seq": 1,
        "action": "authentication_challenge",
        "data": { "token": token }
    });
    write
        .send(Message::Text(auth_msg.to_string()))
        .await
        .context("Failed to send WebSocket authentication")?;

    while let Some(msg) = read.next().await {
        match msg.context("WebSocket read error")? {
            Message::Text(text) => {
                handle_ws_message(state, text).await;
            }
            Message::Ping(data) => {
                if let Err(e) = write.send(Message::Pong(data)).await {
                    warn!("Failed to send WebSocket pong: {:#}", e);
                }
            }
            Message::Close(_) => {
                info!("WebSocket close frame received");
                break;
            }
            _ => {}
        }
    }

    Ok(())
}

async fn handle_ws_message(state: &Arc<AppState>, text: String) {
    let event: WsEvent = match serde_json::from_str(&text) {
        Ok(e) => e,
        Err(e) => {
            warn!("Failed to parse WebSocket message: {:#}", e);
            return;
        }
    };

    if event.event.as_deref() != Some("posted") {
        return;
    }

    let data = match event.data {
        Some(d) => d,
        None => return,
    };

    if data.channel_type.as_deref() != Some("D") {
        return;
    }

    let post_str = match data.post {
        Some(s) => s,
        None => return,
    };

    let post: MmPost = match serde_json::from_str(&post_str) {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to parse post JSON: {:#}", e);
            return;
        }
    };

    if post.user_id == state.bot_user_id {
        return;
    }

    let state_clone = Arc::clone(state);
    tokio::spawn(async move {
        commands::dispatch(
            state_clone,
            post.id,
            post.channel_id,
            post.user_id,
            post.message,
        )
        .await;
    });
}
