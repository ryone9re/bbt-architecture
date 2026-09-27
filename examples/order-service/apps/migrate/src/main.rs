use infra_telemetry::Telemetry;
use std::time::Duration;
use tracing::Instrument;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry = Telemetry::init("bbt-migrate")?;
    let span = tracing::info_span!("migrate.job", otel.status_code = tracing::field::Empty);
    infra_telemetry::set_job_parent(&span);
    let result = infra_runtime::run_job(
        Duration::from_secs(300),
        async {
            let pool =
                infra_postgres::connect(&infra_runtime::required("DATABASE_URL")?, 1, 240_000)
                    .await?;
            let result = infra_postgres::MIGRATOR.run(&pool).await;
            pool.close().await;
            Ok(result?)
        }
        .instrument(span.clone()),
    )
    .await;
    if result.is_err() {
        span.record("otel.status_code", "ERROR");
    }
    drop(span);
    let flushed = telemetry.shutdown();
    result.and(flushed)
}
