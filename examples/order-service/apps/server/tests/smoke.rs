//! Run with `cargo test -p app-server --test smoke -- --ignored --nocapture`.
//! Uses only the dedicated example Compose services and briefly stops Valkey.
#![cfg(unix)]

use anyhow::{Context, Result, ensure};
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use redis::AsyncCommands;
use reqwest::{Client, RequestBuilder, header::HeaderMap};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    fs::File,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};
use uuid::Uuid;

const DATABASE: &str = "postgres://bbt:bbt@127.0.0.1:15432/bbt";
const VALKEY: &str = "redis://127.0.0.1:16379";
const API_KEY: &str = "smoke-test-api-key-at-least-32-bytes";
const BASE: &str = "http://127.0.0.1:13000";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
}
fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command.current_dir(root()).envs([
        ("DATABASE_URL", DATABASE),
        ("VALKEY_URL", VALKEY),
        ("API_KEY", API_KEY),
        ("HTTP_ADDR", "127.0.0.1:13000"),
        ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:14317"),
        ("OTEL_TRACES_SAMPLER", "always_on"),
        ("RUST_LOG", "info,infra_cache=debug"),
    ]);
    command
}
fn run(command: &mut Command) -> Result<Output> {
    let output = command.output()?;
    ensure!(
        output.status.success(),
        "command failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}
fn compose(args: &[&str]) -> Result<()> {
    run(command("docker")
        .args(["compose", "-p", "bbt-order-service"])
        .args(args))?;
    Ok(())
}

// Restore the service on assertion failures as well as successful completion.
struct StoppedValkey(bool);
impl Drop for StoppedValkey {
    fn drop(&mut self) {
        if self.0
            && let Err(error) = compose(&["start", "--wait", "valkey"])
        {
            eprintln!("restore Valkey: {error:#}");
        }
    }
}
struct Server {
    child: Child,
    log: PathBuf,
}
impl Drop for Server {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(status)) if status.success()) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            eprintln!(
                "server log: {}\n{}",
                self.log.display(),
                std::fs::read_to_string(&self.log).unwrap_or_default()
            );
        }
    }
}

async fn response(request: RequestBuilder, expected: u16) -> Result<(Value, HeaderMap)> {
    let response = request.send().await?;
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.bytes().await?;
    ensure!(
        status.as_u16() == expected,
        "HTTP {status}: {}",
        String::from_utf8_lossy(&bytes)
    );
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };
    Ok((value, headers))
}
fn get(client: &Client, path: &str) -> RequestBuilder {
    client.get(format!("{BASE}{path}")).bearer_auth(API_KEY)
}
fn post(client: &Client, path: &str) -> RequestBuilder {
    client.post(format!("{BASE}{path}")).bearer_auth(API_KEY)
}
async fn expired_count(pool: &PgPool) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(*) FROM reservations WHERE NOT confirmed AND expires_at <= clock_timestamp()",
    )
    .fetch_one(pool)
    .await?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the dedicated Docker Compose services; temporarily stops Valkey"]
async fn service_lifecycle() -> Result<()> {
    run(command(env!("CARGO")).args(["build", "--locked", "--workspace", "--bins"]))?;
    let metadata: Value = serde_json::from_slice(
        &run(command(env!("CARGO")).args([
            "metadata",
            "--format-version=1",
            "--no-deps",
            "--locked",
        ]))?
        .stdout,
    )?;
    let bin = Path::new(
        metadata["target_directory"]
            .as_str()
            .context("target directory")?,
    )
    .join("debug");
    run(&mut command(bin.join("migrate")))?;
    let trace_id = Uuid::new_v4().simple().to_string();
    let log_path = bin.join(format!("smoke-{trace_id}.log"));
    let log = File::create(&log_path)?;
    let child = command(bin.join("server"))
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()?;
    let mut server = Server {
        child,
        log: log_path,
    };
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .build()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if response(get(&client, "/readyz"), 204).await.is_ok() {
            break;
        }
        ensure!(Instant::now() < deadline, "server readiness timeout");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    response(client.get(format!("{BASE}/healthz")), 204).await?;
    response(
        client
            .post(format!("{BASE}/reservations"))
            .bearer_auth("invalid")
            .json(&json!({})),
        401,
    )
    .await?;
    let id = Uuid::new_v4();
    let body = json!({"id":id,"sku":"BOOK-001","quantity":3,"unit_price_yen":1200});
    let (reservation, headers) = response(
        post(&client, "/reservations")
            .json(&body)
            .header("traceparent", format!("00-{trace_id}-0123456789abcdef-01")),
        200,
    )
    .await?;
    assert_eq!(headers["x-trace-id"], trace_id);
    assert!(headers.contains_key("x-request-id"));
    assert_eq!(
        response(post(&client, "/reservations").json(&body), 200)
            .await?
            .0,
        reservation
    );
    let mut invalid = body.clone();
    invalid["quantity"] = json!(0);
    response(post(&client, "/reservations").json(&invalid), 422).await?;
    invalid["quantity"] = json!(4);
    response(post(&client, "/reservations").json(&invalid), 409).await?;

    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let request = post(&client, &format!("/reservations/{id}/confirm"));
        requests.spawn(response(request, 200));
    }
    let mut orders = Vec::new();
    while let Some(order) = requests.join_next().await {
        orders.push(order??.0);
    }
    let order = &orders[0];
    assert!(orders.iter().all(|o| o == order));
    assert_eq!(order["total_yen"], 3600);
    let pool = PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(3))
        .connect(DATABASE)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders WHERE reservation_id=$1")
        .bind(id)
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 1);
    let order_id: Uuid = order["id"].as_str().context("order id")?.parse()?;
    let path = format!("/orders/{order_id}");
    for _ in 0..2 {
        assert_eq!(response(get(&client, &path), 200).await?.0, *order);
    }
    let mut cache = redis::Client::open(VALKEY)?
        .get_multiplexed_async_connection()
        .await?;
    let cached: String = cache.get(format!("bbt:orders:v1:{order_id}")).await?;
    assert_eq!(serde_json::from_str::<Value>(&cached)?["id"], order["id"]);
    tokio::time::sleep(Duration::from_secs(11)).await;
    assert_eq!(response(get(&client, &path), 200).await?.0, *order);

    let fallback_id = Uuid::new_v4();
    let mut other = body.clone();
    other["id"] = json!(fallback_id);
    response(post(&client, "/reservations").json(&other), 200).await?;
    let fallback = response(
        post(&client, &format!("/reservations/{fallback_id}/confirm")),
        200,
    )
    .await?
    .0;
    {
        let mut restore = StoppedValkey(true);
        compose(&["stop", "valkey"])?;
        let path = format!(
            "/orders/{}",
            fallback["id"].as_str().context("fallback id")?
        );
        assert_eq!(response(get(&client, &path), 200).await?.0, fallback);
        compose(&["start", "--wait", "valkey"])?;
        restore.0 = false;
    }
    let expired = Uuid::new_v4();
    other["id"] = json!(expired);
    response(post(&client, "/reservations").json(&other), 200).await?;
    sqlx::query("UPDATE reservations SET expires_at='2000-01-01T00:00:00Z' WHERE id=$1 OR id=$2")
        .bind(id)
        .bind(expired)
        .execute(&pool)
        .await?;
    response(
        post(&client, &format!("/reservations/{expired}/confirm")),
        409,
    )
    .await?;
    let before = expired_count(&pool).await?;
    let output = run(command(bin.join("cleanup")).args(["--limit", "1"]))?;
    assert_eq!(before - expired_count(&pool).await?, 1);
    assert!(String::from_utf8_lossy(&output.stdout).contains("deleted=1"));
    assert_eq!(
        response(post(&client, &format!("/reservations/{id}/confirm")), 200)
            .await?
            .0,
        *order
    );

    // Historical fixture for a completed UTC day; all other requests use the public API.
    sqlx::query("UPDATE orders SET confirmed_at='2000-01-02T12:00:00Z' WHERE id=$1")
        .bind(order_id)
        .execute(&pool)
        .await?;
    let expected: (i64, i64) = sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(total_yen),0)::bigint FROM orders WHERE confirmed_at >= '2000-01-02T00:00:00Z' AND confirmed_at < '2000-01-03T00:00:00Z'").fetch_one(&pool).await?;
    let args = ["--from", "2000-01-02", "--until", "2000-01-03"];
    run(command(bin.join("rebuild-sales")).args(args))?;
    let report = response(get(&client, "/sales/2000-01-02"), 200).await?.0;
    run(command(bin.join("rebuild-sales")).args(args))?;
    assert_eq!(
        response(get(&client, "/sales/2000-01-02"), 200).await?.0,
        report
    );
    assert_eq!(
        (
            report["order_count"].as_i64().unwrap(),
            report["total_yen"].as_i64().unwrap()
        ),
        expected
    );
    run(command(bin.join("rebuild-sales")).args([
        "--from",
        "1999-01-01",
        "--until",
        "1999-01-02",
    ]))?;
    let empty = response(get(&client, "/sales/1999-01-01"), 200).await?.0;
    assert_eq!(empty["order_count"], 0);
    assert_eq!(empty["total_yen"], 0);
    pool.close().await;

    kill(
        Pid::from_raw(server.child.id().try_into()?),
        Signal::SIGTERM,
    )?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = server.child.try_wait()? {
            ensure!(status.success(), "server exit: {status}");
            break;
        }
        ensure!(Instant::now() < deadline, "server shutdown timeout");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let logs = std::fs::read_to_string(&server.log)?;
    assert!(
        logs.contains("\"cache.tier\":\"memory\"") && logs.contains("\"cache.tier\":\"valkey\"")
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok((trace, _)) = response(
            client.get(format!("http://127.0.0.1:16686/api/traces/{trace_id}")),
            200,
        )
        .await
            && let Some(spans) = trace["data"][0]["spans"].as_array()
            && let Some(root) = spans.iter().find(|s| s["operationName"] == "http.request")
        {
            assert!(spans.len() >= 3);
            assert!(
                root["references"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["spanID"] == "0123456789abcdef")
            );
            assert!(spans.iter().any(|s| {
                s["references"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["spanID"] == root["spanID"])
            }));
            break;
        }
        ensure!(Instant::now() < deadline, "trace export timeout");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(())
}
