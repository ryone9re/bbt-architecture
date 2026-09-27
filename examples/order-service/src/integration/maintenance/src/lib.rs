use async_trait::async_trait;
use business::{Error, maintenance::ExpiredReservations};
use infra_postgres::PgPool;

pub struct ReservationCleanup(pub PgPool);
#[async_trait]
impl ExpiredReservations for ReservationCleanup {
    #[tracing::instrument(skip_all, fields(otel.kind = "client", db.system.name = "postgresql", deleted))]
    async fn delete_chunk(&self, limit: i64) -> Result<u64, Error> {
        // A single statement is atomic. SKIP LOCKED permits overlapping cron invocations
        // and avoids racing with the lock acquired by order confirmation.
        let result = sqlx::query("WITH expired AS (SELECT id FROM reservations WHERE NOT confirmed AND expires_at <= clock_timestamp() ORDER BY expires_at, id LIMIT $1 FOR UPDATE SKIP LOCKED) DELETE FROM reservations r USING expired e WHERE r.id=e.id")
            .bind(limit).execute(&self.0).await.map_err(|_| {
                tracing::error!("reservation cleanup failed"); Error::Unavailable
            })?;
        tracing::Span::current().record("deleted", result.rows_affected());
        Ok(result.rows_affected())
    }
}
