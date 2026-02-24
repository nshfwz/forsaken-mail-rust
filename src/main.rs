mod address;
mod config;
mod http_api;
mod mail_parser;
mod smtp_server;
mod store;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::http_api::AppState;
use crate::store::Store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logger();
    info!("forsaken-mail-rust v{}", env!("CARGO_PKG_VERSION"));

    let cfg = Arc::new(Config::load());
    let store = Store::new(cfg.max_messages_per_mailbox, cfg.message_ttl_minutes);
    info!("serving embedded static assets");

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let smtp_cfg = cfg.clone();
    let smtp_store = store.clone();
    let smtp_shutdown = shutdown_rx.clone();
    let smtp_task = tokio::spawn(async move {
        if let Err(err) = smtp_server::run(smtp_cfg, smtp_store, smtp_shutdown).await {
            error!("SMTP server stopped with error: {}", err);
        }
    });

    let cleanup_store = store.clone();
    let mut cleanup_shutdown = shutdown_rx.clone();
    let cleanup_task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let removed = cleanup_store.cleanup_expired().await;
                    if removed > 0 {
                        info!("expired messages cleaned: {}", removed);
                    }
                }
                changed = cleanup_shutdown.changed() => {
                    if changed.is_ok() && *cleanup_shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    });

    let app_state = AppState {
        cfg: cfg.clone(),
        store,
    };
    let router = http_api::router(app_state);

    let http_addr = normalize_listen_addr(&cfg.http_addr);
    let listener = TcpListener::bind(&http_addr)
        .await
        .with_context(|| format!("failed to bind HTTP listener on {http_addr}"))?;
    info!("HTTP listening on {}", http_addr);

    let mut http_shutdown = shutdown_rx.clone();
    let http_task = tokio::spawn(async move {
        let server = axum::serve(listener, router).with_graceful_shutdown(async move {
            let _ = http_shutdown.changed().await;
        });

        if let Err(err) = server.await {
            error!("HTTP server stopped with error: {}", err);
        }
    });

    tokio::signal::ctrl_c()
        .await
        .context("failed to listen for shutdown signal")?;
    info!("shutdown signal received");

    if shutdown_tx.send(true).is_err() {
        warn!("failed to broadcast shutdown signal");
    }

    let shutdown_wait = tokio::time::timeout(Duration::from_secs(10), async {
        let _ = http_task.await;
        let _ = smtp_task.await;
        let _ = cleanup_task.await;
    })
    .await;

    if shutdown_wait.is_err() {
        warn!("shutdown timeout reached, exiting");
    }

    Ok(())
}

fn init_logger() {
    let env_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .try_init();
}

fn normalize_listen_addr(addr: &str) -> String {
    if addr.starts_with(':') {
        format!("0.0.0.0{}", addr)
    } else {
        addr.to_string()
    }
}
