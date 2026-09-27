use business::{maintenance::ExpireReservationsOperation, reporting::SalesOperations};
use chrono::NaiveDate;
use clap::Parser;

#[derive(Parser)]
#[command(about = "Delete one bounded chunk of expired reservations; invoke every minute")]
pub struct CleanupArgs {
    #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(i64).range(1..=10_000))]
    pub limit: i64,
}
#[derive(Parser)]
#[command(about = "Rebuild sales for a half-open UTC date range [from, until); retry-safe per day")]
pub struct SalesArgs {
    #[arg(long)]
    pub from: NaiveDate,
    #[arg(long)]
    pub until: NaiveDate,
}

#[tracing::instrument(name = "cleanup.operation", skip_all)]
pub async fn cleanup(
    args: CleanupArgs,
    operation: ExpireReservationsOperation,
) -> anyhow::Result<()> {
    let deleted = operation.execute(args.limit).await?;
    tracing::info!(deleted, limit = args.limit, "cleanup completed");
    println!("deleted={deleted}");
    Ok(())
}
#[tracing::instrument(name = "rebuild_sales.operation", skip_all)]
pub async fn rebuild_sales(args: SalesArgs, operations: SalesOperations) -> anyhow::Result<()> {
    anyhow::ensure!(args.from < args.until, "from must be before until");
    anyhow::ensure!(
        (args.until - args.from).num_days() <= 366,
        "a run may cover at most 366 days"
    );
    anyhow::ensure!(
        args.until <= chrono::Utc::now().date_naive(),
        "until must be today or earlier (UTC)"
    );
    let mut day = args.from;
    while day < args.until {
        let result = operations.rebuild(day).await?;
        tracing::info!(%day, order_count = result.order_count, total_yen = result.total_yen, "day committed");
        println!(
            "day={} order_count={} total_yen={}",
            day, result.order_count, result.total_yen
        );
        day = day
            .succ_opt()
            .ok_or_else(|| anyhow::anyhow!("date overflow"))?;
    }
    Ok(())
}
