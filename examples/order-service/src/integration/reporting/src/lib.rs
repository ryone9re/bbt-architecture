use async_trait::async_trait;
use business::{
    Error,
    reporting::{DailySales, SalesRepository},
};
use chrono::{Datelike, NaiveDate};
use infra_postgres::PgPool;

pub struct PersistentSales(pub PgPool);
#[derive(sqlx::FromRow)]
struct SalesRow {
    day: NaiveDate,
    order_count: i64,
    total_yen: i64,
}
impl From<SalesRow> for DailySales {
    fn from(r: SalesRow) -> Self {
        Self {
            day: r.day,
            order_count: r.order_count,
            total_yen: r.total_yen,
        }
    }
}
fn db_error(_: sqlx::Error) -> Error {
    tracing::error!("sales persistence failed");
    Error::Unavailable
}
#[async_trait]
impl SalesRepository for PersistentSales {
    #[tracing::instrument(skip_all, fields(%day, otel.kind = "client", db.system.name = "postgresql"))]
    async fn rebuild(&self, day: NaiveDate) -> Result<DailySales, Error> {
        let mut tx = self.0.begin().await.map_err(db_error)?;
        // Transaction-scoped lock is released on commit, error, disconnect or cancellation.
        let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(7319, $1)")
            .bind(day.num_days_from_ce())
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        if !locked {
            return Err(Error::Busy);
        }
        // One statement: the aggregate reads one MVCC snapshot, and publishes atomically.
        // Sum is computed in PostgreSQL numeric and range checked when cast to bigint.
        let row: SalesRow = sqlx::query_as("INSERT INTO daily_sales (day, order_count, total_yen) SELECT $1, COUNT(*), COALESCE(SUM(total_yen),0)::bigint FROM orders WHERE confirmed_at >= ($1::date::timestamp AT TIME ZONE 'UTC') AND confirmed_at < (($1::date + 1)::timestamp AT TIME ZONE 'UTC') ON CONFLICT (day) DO UPDATE SET order_count=EXCLUDED.order_count, total_yen=EXCLUDED.total_yen, rebuilt_at=clock_timestamp() RETURNING day, order_count, total_yen")
            .bind(day).fetch_one(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(row.into())
    }
    #[tracing::instrument(skip_all, fields(otel.kind = "client", db.system.name = "postgresql"))]
    async fn find(&self, day: NaiveDate) -> Result<DailySales, Error> {
        sqlx::query_as::<_, SalesRow>(
            "SELECT day, order_count, total_yen FROM daily_sales WHERE day=$1",
        )
        .bind(day)
        .fetch_optional(&self.0)
        .await
        .map_err(db_error)?
        .map(Into::into)
        .ok_or(Error::NotFound)
    }
}
