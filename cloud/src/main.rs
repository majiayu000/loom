use loom_cloud::{App, router};
use std::{path::PathBuf, sync::Arc};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    sqlx::migrate!("./migrations").run(&db).await?;
    let storage = PathBuf::from(std::env::var("LOOM_ARTIFACT_DIR")?);
    anyhow::ensure!(storage.is_absolute(), "LOOM_ARTIFACT_DIR must be absolute");
    tokio::fs::create_dir_all(&storage).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&storage, std::fs::Permissions::from_mode(0o700)).await?;
    }
    if std::env::args().any(|a| a == "--gc") {
        let mut entries = tokio::fs::read_dir(&storage).await?;
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().into_owned();
            if uuid::Uuid::parse_str(&name).is_err() {
                continue;
            }
            let meta = entry.metadata().await?;
            if !meta.is_file() || meta.modified()?.elapsed()?.as_secs() < 86400 {
                continue;
            }
            let referenced: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM skill_versions WHERE artifact_key=$1)",
            )
            .bind(&name)
            .fetch_one(&db)
            .await?;
            if !referenced {
                // New uploads always receive new UUIDs and cannot reference this old key.
                let recheck: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM skill_versions WHERE artifact_key=$1)",
                )
                .bind(&name)
                .fetch_one(&db)
                .await?;
                if !recheck {
                    tokio::fs::remove_file(entry.path()).await?;
                }
            }
        }
        sqlx::query("DELETE FROM idempotency_requests WHERE expires_at<now()")
            .execute(&db)
            .await?;
        return Ok(());
    }
    let jwks_url = std::env::var("LOOM_AUTH_JWKS_URL")?;
    anyhow::ensure!(jwks_url.starts_with("https://"), "JWKS URL must use HTTPS");
    let jwks = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .get(jwks_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let app = App {
        db,
        storage,
        issuer: std::env::var("LOOM_AUTH_ISSUER")?,
        audience: std::env::var("LOOM_AUTH_AUDIENCE")?,
        jwks: Arc::new(jwks),
    };
    let listener = tokio::net::TcpListener::bind(std::env::var("LOOM_BIND")?).await?;
    let origins = std::env::var("LOOM_CORS_ORIGINS")?
        .split(',')
        .map(|s| s.trim().parse())
        .collect::<Result<Vec<axum::http::HeaderValue>, _>>()?;
    anyhow::ensure!(
        !origins.is_empty() && origins.iter().all(|o| o != "*"),
        "Explicit CORS origins required"
    );
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PATCH,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
        ])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderName::from_static("idempotency-key"),
            axum::http::header::IF_MATCH,
        ]);
    axum::serve(listener, router(app).layer(cors))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
