use clap::Parser;
use infra_telemetry::Telemetry;
use std::{sync::Arc, time::Duration};
use tracing::Instrument;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = entry_cli::CleanupArgs::parse();
    let telemetry = Telemetry::init("bbt-cleanup")?;
    let span = tracing::info_span!("cleanup.job", otel.status_code = tracing::field::Empty);
    infra_telemetry::set_job_parent(&span);
    let result = infra_runtime::run_job(
        Duration::from_secs(45),
        async {
            let pool =
                infra_postgres::connect(&infra_runtime::required("DATABASE_URL")?, 2, 10_000)
                    .await?;
            let operation = business::maintenance::ExpireReservationsOperation::new(Arc::new(
                integration_maintenance::ReservationCleanup(pool.clone()),
            ));
            let result = entry_cli::cleanup(args, operation).await;
            pool.close().await;
            result
        }
        .instrument(span.clone()),
    )
    .await;
    if result.is_err() {
        tracing::error!("cleanup failed; retry on the next cron tick");
    }
    if result.is_err() {
        span.record("otel.status_code", "ERROR");
    }
    drop(span);
    let flushed = telemetry.shutdown();
    result.and(flushed)
}
