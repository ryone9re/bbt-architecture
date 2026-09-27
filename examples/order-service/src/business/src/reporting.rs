use crate::Error;
use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct DailySales {
    pub day: NaiveDate,
    pub order_count: i64,
    pub total_yen: i64,
}
#[async_trait]
pub trait SalesRepository: Send + Sync {
    /// Recompute a complete UTC day from a consistent snapshot and replace atomically.
    async fn rebuild(&self, day: NaiveDate) -> Result<DailySales, Error>;
    async fn find(&self, day: NaiveDate) -> Result<DailySales, Error>;
}
#[derive(Clone)]
pub struct SalesOperations(Arc<dyn SalesRepository>);
impl SalesOperations {
    pub fn new(store: Arc<dyn SalesRepository>) -> Self {
        Self(store)
    }
    pub async fn rebuild(&self, day: NaiveDate) -> Result<DailySales, Error> {
        if day >= Utc::now().date_naive() {
            return Err(Error::Invalid("only completed UTC days can be rebuilt"));
        }
        self.0.rebuild(day).await
    }
    pub async fn find(&self, day: NaiveDate) -> Result<DailySales, Error> {
        self.0.find(day).await
    }
}
