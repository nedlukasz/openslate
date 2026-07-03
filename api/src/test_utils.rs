use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

use crate::users::create_first_user;

pub async fn prepare_db() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("failed to create pool");

    sqlx::query(
        "CREATE TABLE users (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(&pool)
    .await
    .expect("prepare_db: failed to create users table");

    pool
}

pub async fn setup_db() -> SqlitePool {
    let pool = prepare_db().await;
    create_first_user(&pool, "test_password")
        .await
        .expect("setup_db: create admin user failed");
    pool
}
