use crate::Error;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait ExpiredReservations: Send + Sync {
    /// Delete at most `limit` expired, unconfirmed reservations atomically.
    async fn delete_chunk(&self, limit: i64) -> Result<u64, Error>;
}

pub struct ExpireReservationsOperation(Arc<dyn ExpiredReservations>);
impl ExpireReservationsOperation {
    pub fn new(store: Arc<dyn ExpiredReservations>) -> Self {
        Self(store)
    }
    pub async fn execute(&self, limit: i64) -> Result<u64, Error> {
        if !(1..=10_000).contains(&limit) {
            return Err(Error::Invalid("limit must be 1..10000"));
        }
        self.0.delete_chunk(limit).await
    }
}
