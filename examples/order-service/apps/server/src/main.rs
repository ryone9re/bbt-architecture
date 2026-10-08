use business::{order::OrderOperations, reporting::SalesOperations};
use entry_http::HttpState;
use infra_telemetry::Telemetry;
use std::{sync::Arc, time::Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry = Telemetry::init("bbt-server")?;
    let result = run().await;
    let flushed = telemetry.shutdown();
    result.and(flushed)
}
async fn run() -> anyhow::Result<()> {
    let api_key = infra_runtime::required("API_KEY")?;
    anyhow::ensure!(
        api_key.len() >= 32,
        "API_KEY must contain at least 32 bytes"
    );
    let pool = infra_postgres::connect(&infra_runtime::required("DATABASE_URL")?, 10, 5000).await?;
    let cache_url = infra_runtime::required("VALKEY_URL")?;
    let cache = match tokio::time::timeout(
        Duration::from_secs(2),
        infra_cache::TieredCache::connect(&cache_url),
    )
    .await
    {
        Ok(Ok(cache)) => Some(Arc::new(cache)),
        _ => {
            tracing::warn!("Valkey unavailable at startup; running with PostgreSQL until restart");
            None
        }
    };
    let state = HttpState {
        orders: OrderOperations::new(Arc::new(integration_orders::PersistentOrders::new(
            pool.clone(),
            cache,
        ))),
        sales: SalesOperations::new(Arc::new(integration_reporting::PersistentSales(
            pool.clone(),
        ))),
        pool: pool.clone(),
    };
    let app = entry_http::router(state, &api_key);
    let address = std::env::var("HTTP_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(address).await?;
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(infra_runtime::shutdown_signal())
        .await;
    pool.close().await;
    Ok(result?)
}
