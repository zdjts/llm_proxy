//! Binary entrypoint for `llm_proxy`.
//!
//! Startup: tracing → [`llm_proxy::bootstrap::bootstrap`] → bind → serve.

use tokio::signal;

use llm_proxy::bootstrap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config_path =
        std::env::var("LLM_PROXY_CONFIG").unwrap_or_else(|_| "config.yaml".to_string());

    let booted = match bootstrap::bootstrap(&config_path).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    tracing::info!("listening on {}", booted.addr);

    let listener = tokio::net::TcpListener::bind(booted.addr).await?;
    let shutdown_tx = booted.shutdown_tx;
    let health_handle = booted.health_handle;

    axum::serve(listener, booted.app)
        .with_graceful_shutdown(async move {
            let _ = signal::ctrl_c().await;
            tracing::info!("shutting down...");
            let _ = shutdown_tx.send(());
        })
        .await?;

    let _ = tokio::time::timeout(std::time::Duration::from_secs(30), health_handle).await;

    tracing::info!("llm_proxy stopped");
    Ok(())
}
