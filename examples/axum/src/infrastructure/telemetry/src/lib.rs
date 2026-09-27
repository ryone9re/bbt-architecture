use opentelemetry::{
    global,
    propagation::Extractor,
    trace::{TraceContextExt, TracerProvider},
};
use opentelemetry_sdk::{Resource, propagation::TraceContextPropagator, trace::SdkTracerProvider};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub struct Telemetry(SdkTracerProvider);
impl Telemetry {
    pub fn init(service: &'static str) -> anyhow::Result<Self> {
        global::set_text_map_propagator(TraceContextPropagator::new());
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .build()?;
        let provider = SdkTracerProvider::builder()
            .with_resource(Resource::builder().with_service_name(service).build())
            .with_batch_exporter(exporter)
            .build();
        let tracer = provider.tracer("bbt-example");
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .with(tracing_subscriber::fmt::layer().json())
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .try_init()?;
        Ok(Self(provider))
    }
    /// Call while the Tokio runtime is alive, including on application errors.
    pub fn shutdown(self) -> anyhow::Result<()> {
        self.0.shutdown().map_err(Into::into)
    }
}

pub fn set_parent(span: &tracing::Span, carrier: &impl Extractor) {
    let context = global::get_text_map_propagator(|p| p.extract(carrier));
    let _ = span.set_parent(context);
}
pub fn trace_id(span: &tracing::Span) -> String {
    span.context().span().span_context().trace_id().to_string()
}
/// Propagate an orchestrator's TRACEPARENT into a batch root span when supplied.
pub fn set_job_parent(span: &tracing::Span) {
    let carrier = std::collections::HashMap::<String, String>::from_iter(
        std::env::var("TRACEPARENT")
            .ok()
            .map(|v| ("traceparent".to_owned(), v)),
    );
    set_parent(span, &carrier);
}
pub use opentelemetry::propagation::Extractor as TraceExtractor;
