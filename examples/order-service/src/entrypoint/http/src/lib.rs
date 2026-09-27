use axum::{
    Extension, Json, Router,
    error_handling::HandleErrorLayer,
    extract::{DefaultBodyLimit, MatchedPath, Path, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::AUTHORIZATION},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use business::{
    Error,
    order::{Order, OrderOperations, Reservation, ReserveInput},
    reporting::{DailySales, SalesOperations},
};
use chrono::{DateTime, NaiveDate, Utc};
use infra_postgres::PgPool;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tower::{ServiceBuilder, limit::GlobalConcurrencyLimitLayer, load_shed::LoadShedLayer};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
};
use tracing::Instrument;
use uuid::Uuid;

#[derive(Clone)]
pub struct HttpState {
    pub orders: OrderOperations,
    pub sales: SalesOperations,
    pub pool: PgPool,
}
#[derive(Clone)]
struct ApiKey([u8; 32]);
#[derive(Clone)]
struct Principal {
    subject: &'static str,
}

pub fn router(state: HttpState, api_key: &str) -> Router {
    let auth = Arc::new(ApiKey(Sha256::digest(api_key.as_bytes()).into()));
    let business_routes = Router::new()
        .route("/reservations", post(reserve))
        .route("/reservations/{id}/confirm", post(confirm))
        .route("/orders/{id}", get(find_order))
        .route("/sales/{day}", get(find_sales))
        // Only the business routes require credentials. Authentication adds Principal.
        .route_layer(middleware::from_fn_with_state(auth, authenticate));
    Router::new()
        .merge(business_routes)
        .route("/healthz", get(|| async { StatusCode::NO_CONTENT }))
        .route("/readyz", get(ready))
        .with_state(state)
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(
            ServiceBuilder::new()
                // Request execution order is top -> bottom; responses unwind in reverse.
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(PropagateRequestIdLayer::x_request_id())
                .layer(middleware::from_fn(trace_request))
                .layer(TimeoutLayer::with_status_code(
                    StatusCode::REQUEST_TIMEOUT,
                    Duration::from_secs(10),
                ))
                .layer(HandleErrorLayer::new(|_: tower::BoxError| async {
                    StatusCode::SERVICE_UNAVAILABLE
                }))
                .layer(LoadShedLayer::new())
                .layer(GlobalConcurrencyLimitLayer::new(128)),
        )
}

async fn authenticate(
    State(key): State<Arc<ApiKey>>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let accepted = token
        .map(|token| {
            let actual: [u8; 32] = Sha256::digest(token.as_bytes()).into();
            bool::from(actual.ct_eq(&key.0))
        })
        .unwrap_or(false);
    if !accepted {
        return (
            StatusCode::UNAUTHORIZED,
            [("www-authenticate", "Bearer")],
            Json(serde_json::json!({"error":"unauthorized"})),
        )
            .into_response();
    }
    if let Some(value) = request.headers_mut().get_mut(AUTHORIZATION) {
        value.set_sensitive(true);
    }
    request.extensions_mut().insert(Principal {
        subject: "internal-service",
    });
    next.run(request).await
}

struct Headers<'a>(&'a HeaderMap);
impl infra_telemetry::TraceExtractor for Headers<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key)?.to_str().ok()
    }
    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}
async fn trace_request(request: Request, next: Next) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str())
        .unwrap_or("unmatched");
    let span = tracing::info_span!("http.request", otel.kind = "server", http.request.method = %request.method(), http.route = route,
        http.response.status_code = tracing::field::Empty, otel.status_code = tracing::field::Empty,
        request_id = request.headers().get("x-request-id").and_then(|v| v.to_str().ok()).unwrap_or(""));
    infra_telemetry::set_parent(&span, &Headers(request.headers()));
    let trace_id = infra_telemetry::trace_id(&span);
    let start = Instant::now();
    let mut response = next.run(request).instrument(span.clone()).await;
    span.record("http.response.status_code", response.status().as_u16());
    if response.status().is_server_error() {
        span.record("otel.status_code", "ERROR");
    }
    span.in_scope(|| tracing::info!(elapsed_ms = start.elapsed().as_millis() as u64, %trace_id, "request completed"));
    if let Ok(value) = HeaderValue::from_str(&trace_id) {
        response.headers_mut().insert("x-trace-id", value);
    }
    response
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReserveBody {
    id: Uuid,
    sku: String,
    quantity: i32,
    unit_price_yen: i64,
}
#[derive(Serialize)]
struct ReservationBody {
    id: Uuid,
    sku: String,
    quantity: i32,
    unit_price_yen: i64,
    expires_at: DateTime<Utc>,
}
impl From<Reservation> for ReservationBody {
    fn from(r: Reservation) -> Self {
        Self {
            id: r.id,
            sku: r.sku,
            quantity: r.quantity,
            unit_price_yen: r.unit_price_yen,
            expires_at: r.expires_at,
        }
    }
}
#[derive(Serialize)]
struct OrderBody {
    id: Uuid,
    reservation_id: Uuid,
    sku: String,
    quantity: i32,
    total_yen: i64,
    confirmed_at: DateTime<Utc>,
}
impl From<Order> for OrderBody {
    fn from(o: Order) -> Self {
        Self {
            id: o.id,
            reservation_id: o.reservation_id,
            sku: o.sku,
            quantity: o.quantity,
            total_yen: o.total_yen,
            confirmed_at: o.confirmed_at,
        }
    }
}
#[derive(Serialize)]
struct SalesBody {
    day: NaiveDate,
    order_count: i64,
    total_yen: i64,
}
impl From<DailySales> for SalesBody {
    fn from(s: DailySales) -> Self {
        Self {
            day: s.day,
            order_count: s.order_count,
            total_yen: s.total_yen,
        }
    }
}

#[tracing::instrument(skip_all)]
async fn reserve(
    State(state): State<HttpState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<ReserveBody>,
) -> Result<Json<ReservationBody>, ApiError> {
    tracing::info!(
        subject = principal.subject,
        operation = "reserve",
        "authenticated operation"
    );
    let result = state
        .orders
        .reserve(ReserveInput {
            id: body.id,
            sku: body.sku,
            quantity: body.quantity,
            unit_price_yen: body.unit_price_yen,
        })
        .await?;
    Ok(Json(result.into()))
}
#[tracing::instrument(skip_all)]
async fn confirm(
    State(state): State<HttpState>,
    Path(id): Path<Uuid>,
) -> Result<Json<OrderBody>, ApiError> {
    Ok(Json(state.orders.confirm(id).await?.into()))
}
#[tracing::instrument(skip_all)]
async fn find_order(
    State(state): State<HttpState>,
    Path(id): Path<Uuid>,
) -> Result<Json<OrderBody>, ApiError> {
    Ok(Json(state.orders.find(id).await?.into()))
}
async fn find_sales(
    State(state): State<HttpState>,
    Path(day): Path<NaiveDate>,
) -> Result<Json<SalesBody>, ApiError> {
    Ok(Json(state.sales.find(day).await?.into()))
}
async fn ready(State(state): State<HttpState>) -> StatusCode {
    match tokio::time::timeout(Duration::from_secs(1), sqlx_ping(&state.pool)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}
async fn sqlx_ping(pool: &PgPool) -> Result<(), ()> {
    use infra_postgres::Connection;
    pool.acquire()
        .await
        .map_err(|_| ())?
        .ping()
        .await
        .map_err(|_| ())
}
struct ApiError(Error);
impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        Self(error)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.0 {
            Error::Invalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Expired | Error::Conflict | Error::Busy => StatusCode::CONFLICT,
            Error::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        (
            status,
            Json(serde_json::json!({"error": self.0.to_string()})),
        )
            .into_response()
    }
}
