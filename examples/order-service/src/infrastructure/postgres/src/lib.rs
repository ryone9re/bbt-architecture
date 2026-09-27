pub use sqlx::PgPool;
use sqlx::{
    ConnectOptions,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::time::Duration;
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub async fn connect(
    url: &str,
    max_connections: u32,
    statement_timeout_ms: u32,
) -> anyhow::Result<PgPool> {
    // SQL arguments may contain sensitive data; instrument named operations in Integration.
    let options: PgConnectOptions = url.parse()?;
    Ok(PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(3))
        .after_connect(move |conn, _| Box::pin(async move {
            sqlx::query("SELECT set_config('statement_timeout', $1, false), set_config('lock_timeout', '2000', false), set_config('idle_in_transaction_session_timeout', '10000', false)")
                .bind(statement_timeout_ms.to_string()).execute(conn).await?;
            Ok(())
        }))
        .connect_with(options.disable_statement_logging()).await?)
}
pub use sqlx::Connection;
