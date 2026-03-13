use std::sync::Arc;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use kassi_server::config::Config;
use kassi_server::{app, AppState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();

    let config = Config::from_env();

    let db = kassi_db::create_pool(&config.database_url)
        .await
        .expect("failed to create database pool");

    let kms = kassi_signer::InfisicalKms::login(
        &config.infisical_client_id,
        &config.infisical_client_secret,
        &config.infisical_project_id,
    )
    .await
    .expect("failed to authenticate with infisical KMS");

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = TcpListener::bind(addr)
        .await
        .expect("failed to bind address");

    tracing::info!("listening on {addr}");

    let state = AppState {
        db,
        config,
        kms: Some(Arc::new(kms)),
    };

    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install ctrl+c handler");
    tracing::info!("shutting down");
}
