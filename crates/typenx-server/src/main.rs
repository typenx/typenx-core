use std::{env, net::SocketAddr, sync::Arc};

use typenx_server::{build_router, AppState};
use typenx_storage::{SqlStore, TypenxStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let database_url = env::var("TYPENX_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://typenx.sqlite?mode=rwc".to_owned());
    let bind_addr: SocketAddr = env::var("TYPENX_BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
        .parse()?;

    let store = SqlStore::connect(&database_url).await?;
    store.migrate().await?;

    let state = AppState::new(Arc::new(store));
    state
        .seed_default_addons()
        .await
        .map_err(|error| anyhow::anyhow!("failed to seed default addons: {error:?}"))?;
    let router = build_router(state);
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "failed to bind Typenx server to {bind_addr}: {error}. \
                 If another Typenx server is already running, stop it or set \
                 TYPENX_BIND_ADDR to a free address."
            )
        })?;
    axum::serve(listener, router).await?;
    Ok(())
}
