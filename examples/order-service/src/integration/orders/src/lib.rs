use async_trait::async_trait;
use business::{
    Error,
    order::{Order, OrderRepository, Reservation, ReserveInput},
};
use chrono::{DateTime, TimeDelta, Utc};
use infra_cache::TieredCache;
use infra_postgres::PgPool;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

pub struct PersistentOrders {
    pool: PgPool,
    cache: Option<Arc<TieredCache>>,
}
impl PersistentOrders {
    pub fn new(pool: PgPool, cache: Option<Arc<TieredCache>>) -> Self {
        Self { pool, cache }
    }
}

#[derive(sqlx::FromRow)]
struct ReservationRow {
    id: Uuid,
    sku: String,
    quantity: i32,
    unit_price_yen: i64,
    expires_at: DateTime<Utc>,
}
impl From<ReservationRow> for Reservation {
    fn from(r: ReservationRow) -> Self {
        Self {
            id: r.id,
            sku: r.sku,
            quantity: r.quantity,
            unit_price_yen: r.unit_price_yen,
            expires_at: r.expires_at,
        }
    }
}

#[derive(Clone, sqlx::FromRow, Serialize, Deserialize)]
struct OrderRecord {
    id: Uuid,
    reservation_id: Uuid,
    sku: String,
    quantity: i32,
    total_yen: i64,
    confirmed_at: DateTime<Utc>,
}
impl From<OrderRecord> for Order {
    fn from(r: OrderRecord) -> Self {
        Self {
            id: r.id,
            reservation_id: r.reservation_id,
            sku: r.sku,
            quantity: r.quantity,
            total_yen: r.total_yen,
            confirmed_at: r.confirmed_at,
        }
    }
}
fn db_error(error: sqlx::Error) -> Error {
    // Deliberately do not log raw database errors: detail strings can contain input values.
    tracing::error!(error_class = ?std::mem::discriminant(&error), "order persistence failed");
    Error::Unavailable
}

#[async_trait]
impl OrderRepository for PersistentOrders {
    #[tracing::instrument(name = "reserve", skip_all, fields(otel.kind = "client", db.system.name = "postgresql"))]
    async fn reserve(
        &self,
        input: ReserveInput,
        lifetime: TimeDelta,
    ) -> Result<Reservation, Error> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        // ON CONFLICT waits for concurrent inserts. A subsequent statement gets a fresh snapshot.
        sqlx::query("INSERT INTO reservations (id, sku, quantity, unit_price_yen, expires_at) VALUES ($1,$2,$3,$4,clock_timestamp() + $5 * interval '1 second') ON CONFLICT (id) DO NOTHING")
            .bind(input.id).bind(&input.sku).bind(input.quantity).bind(input.unit_price_yen)
            .bind(lifetime.num_seconds() as f64).execute(&mut *tx).await.map_err(db_error)?;
        let row: ReservationRow = sqlx::query_as("SELECT id, sku, quantity, unit_price_yen, expires_at FROM reservations WHERE id=$1 FOR UPDATE")
            .bind(input.id).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
        if row.sku != input.sku
            || row.quantity != input.quantity
            || row.unit_price_yen != input.unit_price_yen
        {
            return Err(Error::Conflict);
        }
        tx.commit().await.map_err(db_error)?;
        Ok(row.into())
    }

    #[tracing::instrument(name = "confirm", skip_all, fields(otel.kind = "client", db.system.name = "postgresql"))]
    async fn confirm(&self, id: Uuid) -> Result<Order, Error> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let reservation: ReservationRow = sqlx::query_as("SELECT id, sku, quantity, unit_price_yen, expires_at FROM reservations WHERE id=$1 FOR UPDATE")
            .bind(id).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::NotFound)?;
        if let Some(existing) = sqlx::query_as::<_, OrderRecord>("SELECT id, reservation_id, sku, quantity, total_yen, confirmed_at FROM orders WHERE reservation_id=$1")
            .bind(id).fetch_optional(&mut *tx).await.map_err(db_error)? {
            tx.commit().await.map_err(db_error)?;
            return Ok(existing.into());
        }
        let now = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        let reservation: Reservation = reservation.into();
        let total = reservation.confirmation_total(now)?;
        let order: OrderRecord = sqlx::query_as("INSERT INTO orders (id, reservation_id, sku, quantity, total_yen) VALUES ($1,$2,$3,$4,$5) RETURNING id, reservation_id, sku, quantity, total_yen, confirmed_at")
            .bind(Uuid::new_v4()).bind(id).bind(reservation.sku).bind(reservation.quantity).bind(total)
            .fetch_one(&mut *tx).await.map_err(db_error)?;
        sqlx::query("UPDATE reservations SET confirmed=true WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(order.into())
    }

    #[tracing::instrument(name = "find_order", skip_all)]
    async fn find(&self, id: Uuid) -> Result<Order, Error> {
        let key = format!("bbt:orders:v1:{id}");
        if let Some(cache) = &self.cache
            && let Some(bytes) = cache.get(&key).await
        {
            match serde_json::from_slice::<OrderRecord>(&bytes) {
                Ok(record) if record.id == id => return Ok(record.into()),
                _ => tracing::warn!("invalid cache record; falling back to PostgreSQL"),
            }
        }
        let record = load_order(&self.pool, id).await?;
        if let Some(cache) = &self.cache {
            match serde_json::to_vec(&record) {
                Ok(bytes) => cache.put(&key, bytes).await,
                Err(_) => tracing::warn!("cache serialization failed"),
            }
        }
        Ok(record.into())
    }
}

#[tracing::instrument(skip_all, fields(otel.kind = "client", db.system.name = "postgresql"))]
async fn load_order(pool: &PgPool, id: Uuid) -> Result<OrderRecord, Error> {
    sqlx::query_as(
        "SELECT id, reservation_id, sku, quantity, total_yen, confirmed_at FROM orders WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_error)?
    .ok_or(Error::NotFound)
}
