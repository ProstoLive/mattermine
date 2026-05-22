use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};

#[allow(dead_code)]
pub struct PasswordRecord {
    pub username: String,
    pub hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    let sql = tokio::fs::read_to_string("./migrations/001_initial.sql")
        .await
        .context("Cannot read migrations/001_initial.sql")?;
    sqlx::query(&sql)
        .execute(pool)
        .await
        .context("Failed to execute migration")?;
    tracing::info!("Migrations applied successfully");
    Ok(())
}

pub async fn upsert_password(pool: &PgPool, username: &str, hash: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO minecraft_passwords (username, hash, updated_at)
         VALUES ($1, $2, now())
         ON CONFLICT (username) DO UPDATE
           SET hash = EXCLUDED.hash,
               updated_at = now()",
    )
    .bind(username)
    .bind(hash)
    .execute(pool)
    .await
    .context("Failed to upsert password")?;
    Ok(())
}

pub async fn get_password_record(pool: &PgPool, username: &str) -> Result<Option<PasswordRecord>> {
    let row = sqlx::query(
        "SELECT username, hash, created_at, updated_at
         FROM minecraft_passwords
         WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await
    .context("Failed to query password record")?;

    Ok(row.map(|r| PasswordRecord {
        username: r.get("username"),
        hash: r.get("hash"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }))
}
