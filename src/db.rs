use anyhow::{Context, Result};
use sqlx::PgPool;

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
