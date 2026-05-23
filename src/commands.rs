use crate::{crypto, db, AppState};
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{error, warn};

const BLOCK_THRESHOLD: u32 = 5;
const BLOCK_DURATION: Duration = Duration::from_secs(600);

struct FailState {
    count: u32,
    blocked_since: Option<Instant>,
}

pub struct RateLimiter {
    state: Arc<Mutex<HashMap<String, FailState>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn is_blocked(&self, username: &str) -> bool {
        let mut map = self.state.lock().await;
        let Some(entry) = map.get_mut(username) else {
            return false;
        };
        let Some(since) = entry.blocked_since else {
            return false;
        };
        if since.elapsed() >= BLOCK_DURATION {
            map.remove(username);
            return false;
        }
        true
    }

    pub async fn record_failure(&self, username: &str) {
        let mut map = self.state.lock().await;
        let entry = map.entry(username.to_string()).or_insert(FailState {
            count: 0,
            blocked_since: None,
        });
        entry.count += 1;
        if entry.count >= BLOCK_THRESHOLD && entry.blocked_since.is_none() {
            entry.blocked_since = Some(Instant::now());
        }
    }

    pub async fn reset(&self, username: &str) {
        self.state.lock().await.remove(username);
    }
}

pub enum Command {
    SetPassword(String),
    Unknown,
}

pub fn parse_command(text: &str) -> Command {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("set-password ") {
        let pw = rest.trim();
        if !pw.is_empty() {
            return Command::SetPassword(pw.to_string());
        }
    }
    Command::Unknown
}

async fn send_message(state: &AppState, channel_id: &str, message: &str) -> Result<()> {
    state
        .http
        .post(format!("{}/api/v4/posts", state.mm_url))
        .bearer_auth(&state.mm_token)
        .json(&json!({ "channel_id": channel_id, "message": message }))
        .send()
        .await
        .context("HTTP request to post message failed")?
        .error_for_status()
        .context("Mattermost returned error status for post message")?;
    Ok(())
}

async fn delete_post(state: &AppState, post_id: &str) {
    let result = state
        .http
        .delete(format!("{}/api/v4/posts/{}", state.mm_url, post_id))
        .bearer_auth(&state.mm_token)
        .send()
        .await;
    match result {
        Err(e) => warn!("Failed to delete post {}: {:#}", post_id, e),
        Ok(resp) if !resp.status().is_success() => {
            warn!("Delete post {} returned status {}", post_id, resp.status())
        }
        Ok(_) => {}
    }
}

pub async fn resolve_username(state: &Arc<AppState>, user_id: &str) -> Result<String> {
    let cached = {
        let cache = state.user_cache.lock().await;
        cache.get(user_id).cloned()
    };
    if let Some(username) = cached {
        return Ok(username);
    }

    let resp: serde_json::Value = state
        .http
        .get(format!("{}/api/v4/users/{}", state.mm_url, user_id))
        .bearer_auth(&state.mm_token)
        .send()
        .await
        .context("Failed to fetch user info")?
        .error_for_status()
        .context("Mattermost returned error for user lookup")?
        .json()
        .await
        .context("Failed to parse user info response")?;

    let username = resp["username"]
        .as_str()
        .context("Missing username field in user response")?
        .to_string();

    state
        .user_cache
        .lock()
        .await
        .insert(user_id.to_string(), username.clone());

    Ok(username)
}

async fn handle_set_password(
    state: Arc<AppState>,
    username: String,
    password: String,
    post_id: String,
    channel_id: String,
) {
    if state.rate_limiter.is_blocked(&username).await {
        let _ = send_message(
            &state,
            &channel_id,
            "You are temporarily blocked from setting a password. Please try again in 10 minutes.",
        )
        .await;
        delete_post(&state, &post_id).await;
        return;
    }

    let hash = match crypto::hash_password(&password) {
        Ok(h) => h,
        Err(e) => {
            error!("Failed to hash password for {}: {:#}", username, e);
            let _ = send_message(&state, &channel_id, "Internal error hashing password.").await;
            delete_post(&state, &post_id).await;
            return;
        }
    };

    if let Err(e) = db::upsert_password(&state.pool, &username, &hash).await {
        error!("DB error upserting password for {}: {:#}", username, e);
        let _ = send_message(
            &state,
            &channel_id,
            "Database error storing password. Please try again.",
        )
        .await;
        delete_post(&state, &post_id).await;
        return;
    }

    state.rate_limiter.reset(&username).await;
    let _ = send_message(&state, &channel_id, "Password set successfully.").await;
    delete_post(&state, &post_id).await;
}


pub async fn dispatch(
    state: Arc<AppState>,
    post_id: String,
    channel_id: String,
    user_id: String,
    text: String,
) {
    let username = match resolve_username(&state, &user_id).await {
        Ok(u) => u,
        Err(e) => {
            error!("Failed to resolve username for user_id {}: {:#}", user_id, e);
            return;
        }
    };

    match parse_command(&text) {
        Command::SetPassword(pw) => {
            handle_set_password(state, username, pw, post_id, channel_id).await;
        }
        Command::Unknown => {
            let _ = send_message(
                &state,
                &channel_id,
                "Unknown command. Use `set-password <password>` to set your Minecraft password.",
            )
            .await;
        }
    }
}
