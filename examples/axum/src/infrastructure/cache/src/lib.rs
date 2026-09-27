use moka::future::Cache;
use redis::{
    AsyncCommands,
    aio::{ConnectionManager, ConnectionManagerConfig},
};
use std::time::Duration;

/// Technology-only byte cache. The boundary owns keys and serialization.
#[derive(Clone)]
pub struct TieredCache {
    local: Cache<String, Vec<u8>>,
    remote: ConnectionManager,
}
impl TieredCache {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let config = ConnectionManagerConfig::new()
            .set_connection_timeout(Some(Duration::from_millis(200)))
            .set_response_timeout(Some(Duration::from_millis(100)))
            .set_number_of_retries(1);
        let remote = redis::Client::open(url)?
            .get_connection_manager_with_config(config)
            .await?;
        Ok(Self {
            local: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(10))
                .build(),
            remote,
        })
    }
    #[tracing::instrument(skip_all, fields(otel.kind = "client", db.system.name = "redis", cache.tier))]
    pub async fn get(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(value) = self.local.get(key).await {
            tracing::Span::current().record("cache.tier", "memory");
            tracing::debug!("cache hit");
            return Some(value);
        }
        let result = tokio::time::timeout(
            Duration::from_millis(150),
            self.remote.clone().get::<_, Option<Vec<u8>>>(key),
        )
        .await;
        match result {
            Ok(Ok(Some(value))) => {
                tracing::Span::current().record("cache.tier", "valkey");
                self.local.insert(key.to_owned(), value.clone()).await;
                tracing::debug!("cache hit");
                Some(value)
            }
            Ok(Ok(None)) => {
                tracing::Span::current().record("cache.tier", "miss");
                None
            }
            _ => {
                tracing::warn!("cache read failed; falling back to PostgreSQL");
                None
            }
        }
    }
    #[tracing::instrument(skip_all, fields(otel.kind = "client", db.system.name = "redis"))]
    pub async fn put(&self, key: &str, value: Vec<u8>) {
        self.local.insert(key.to_owned(), value.clone()).await;
        let result = tokio::time::timeout(
            Duration::from_millis(150),
            self.remote.clone().set_ex::<_, _, ()>(key, value, 60),
        )
        .await;
        if !matches!(result, Ok(Ok(()))) {
            tracing::warn!("cache write failed");
        }
    }
}
