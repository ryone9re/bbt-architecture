use crate::Error;
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Reservation {
    pub id: Uuid,
    pub sku: String,
    pub quantity: i32,
    pub unit_price_yen: i64,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct Order {
    pub id: Uuid,
    pub reservation_id: Uuid,
    pub sku: String,
    pub quantity: i32,
    pub total_yen: i64,
    pub confirmed_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct ReserveInput {
    /// Client-generated UUID: retries reuse the same value.
    pub id: Uuid,
    pub sku: String,
    pub quantity: i32,
    pub unit_price_yen: i64,
}

#[async_trait]
pub trait OrderRepository: Send + Sync {
    /// Atomically insert, or return the original reservation if the input matches.
    async fn reserve(&self, input: ReserveInput, lifetime: TimeDelta)
    -> Result<Reservation, Error>;
    /// Atomically check expiry and create one immutable order per reservation.
    /// Repeated confirmation returns the same order, including after expiry.
    async fn confirm(&self, id: Uuid) -> Result<Order, Error>;
    async fn find(&self, id: Uuid) -> Result<Order, Error>;
}

#[derive(Clone)]
pub struct OrderOperations(Arc<dyn OrderRepository>);
impl OrderOperations {
    pub fn new(repository: Arc<dyn OrderRepository>) -> Self {
        Self(repository)
    }

    pub async fn reserve(&self, input: ReserveInput) -> Result<Reservation, Error> {
        if input.id.is_nil()
            || input.sku.is_empty()
            || input.sku.len() > 64
            || !input
                .sku
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err(Error::Invalid(
                "id must be non-nil; sku must be 1..64 ASCII letters, digits, - or _",
            ));
        }
        if !(1..=1000).contains(&input.quantity)
            || !(1..=1_000_000_000).contains(&input.unit_price_yen)
        {
            return Err(Error::Invalid(
                "quantity must be 1..1000; unit_price_yen must be 1..1000000000",
            ));
        }
        self.0.reserve(input, TimeDelta::minutes(15)).await
    }
    pub async fn confirm(&self, id: Uuid) -> Result<Order, Error> {
        self.0.confirm(id).await
    }
    pub async fn find(&self, id: Uuid) -> Result<Order, Error> {
        self.0.find(id).await
    }
}

impl Reservation {
    /// Business policy evaluated with the persistence clock while holding the reservation lock.
    pub fn confirmation_total(&self, now: DateTime<Utc>) -> Result<i64, Error> {
        if self.expires_at <= now {
            return Err(Error::Expired);
        }
        i64::from(self.quantity)
            .checked_mul(self.unit_price_yen)
            .ok_or(Error::Invalid("order total overflow"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_observes_expiry_and_integer_money() {
        let now = Utc::now();
        let mut reservation = Reservation {
            id: Uuid::new_v4(),
            sku: "book".into(),
            quantity: 3,
            unit_price_yen: 1200,
            expires_at: now + TimeDelta::seconds(1),
        };
        assert_eq!(reservation.confirmation_total(now).unwrap(), 3600);
        reservation.expires_at = now;
        assert!(matches!(
            reservation.confirmation_total(now),
            Err(Error::Expired)
        ));
    }
}
