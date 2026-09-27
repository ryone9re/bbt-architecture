use std::{future::Future, time::Duration};

pub fn required(name: &str) -> anyhow::Result<String> {
    let value = std::env::var(name).map_err(|_| anyhow::anyhow!("{name} is required"))?;
    anyhow::ensure!(!value.trim().is_empty(), "{name} must not be empty");
    Ok(value)
}

pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .expect("install Ctrl-C handler");
}

/// Dropping the future rolls back an open SQLx transaction; completed chunks remain committed.
pub async fn run_job<T>(
    duration: Duration,
    job: impl Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    tokio::select! {
        result = tokio::time::timeout(duration, job) => result.map_err(|_| anyhow::anyhow!("job deadline exceeded"))?,
        _ = shutdown_signal() => Err(anyhow::anyhow!("job interrupted; safe to retry")),
    }
}
