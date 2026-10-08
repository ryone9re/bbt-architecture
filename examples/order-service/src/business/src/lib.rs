//! Business contracts use no transport, persistence, cache, or telemetry types.
pub mod maintenance;
pub mod order;
pub mod reporting;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("not found")]
    NotFound,
    #[error("reservation expired")]
    Expired,
    #[error("idempotency key already used with different input")]
    Conflict,
    #[error("operation already running")]
    Busy,
    #[error("dependency unavailable")]
    Unavailable,
}
