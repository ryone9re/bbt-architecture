use clap::Parser;
use infra_telemetry::Telemetry;
use std::{sync::Arc, time::Duration};
use tracing::Instrument;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = entry_cli::SalesArgs::parse();
    let telemetry = Telemetry::init("bbt-rebuild-sales")?;
    let span = tracing::info_span!(
        "rebuild_sales.job",
        otel.status_code = tracing::field::Empty
    );
    infra_telemetry::set_job_parent(&span);
    let result = infra_runtime::run_job(
        Duration::from_secs(3300),
        async {
            let pool =
                infra_postgres::connect(&infra_runtime::required("DATABASE_URL")?, 2, 300_000)
                    .await?;
            let operations = business::reporting::SalesOperations::new(Arc::new(
                integration_reporting::PersistentSales(pool.clone()),
            ));
            let result = entry_cli::rebuild_sales(args, operations).await;
            pool.close().await;
            result
        }
        .instrument(span.clone()),
    )
    .await;
    if result.is_err() {
        tracing::error!("sales rebuild failed; rerun the same range");
    }
    if result.is_err() {
        span.record("otel.status_code", "ERROR");
    }
    drop(span);
    let flushed = telemetry.shutdown();
    result.and(flushed)
}
