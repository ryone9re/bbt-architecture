#!/usr/bin/env python3
"""Exercise real HTTP, PostgreSQL, Valkey, batch executables and OTLP export.

Run against the example's dedicated local Compose project. Creates test orders;
briefly stops that project's Valkey to exercise the PostgreSQL fallback.
"""
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
ENV = os.environ | {
    "DATABASE_URL": "postgres://bbt:bbt@127.0.0.1:15432/bbt",
    "VALKEY_URL": "redis://127.0.0.1:16379",
    "API_KEY": "smoke-test-api-key-at-least-32-bytes",
    "HTTP_ADDR": "127.0.0.1:13000",
    "OTEL_EXPORTER_OTLP_ENDPOINT": "http://127.0.0.1:14317",
    "OTEL_TRACES_SAMPLER": "always_on",
    "RUST_LOG": "info,infra_cache=debug",
}
BASE = "http://127.0.0.1:13000"
BIN = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug"


def run(*args, **kwargs):
    return subprocess.run(args, cwd=ROOT, env=ENV, text=True, check=True, **kwargs)


def sql(statement):
    return run("docker", "compose", "exec", "-T", "postgres", "psql", "-U", "bbt", "-d", "bbt", "-v", "ON_ERROR_STOP=1", "-Atc", statement, capture_output=True).stdout.strip()


def http(path, body=None, method=None, token=ENV["API_KEY"], headers=None, expected=200):
    request = urllib.request.Request(BASE + path, method=method, headers={
        "Authorization": "Bearer " + token, "Content-Type": "application/json", **(headers or {}),
    }, data=json.dumps(body).encode() if body is not None else None)
    try:
        response = urllib.request.urlopen(request, timeout=15)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        data = response.read()
        assert response.status == expected, (path, response.status, data)
        return json.loads(data) if data and response.headers.get("content-type", "").startswith("application/json") else data, response.headers


def wait_for(check, seconds=30):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            result = check()
            if result:
                return result
        except (OSError, AssertionError, urllib.error.URLError) as error:
            last = error
        time.sleep(0.25)
    raise AssertionError(f"condition timed out: {last}")


def main():
    run("cargo", "build", "--locked", "--workspace", "--bins")
    run(str(BIN / "migrate"))
    trace_id = uuid.uuid4().hex
    with tempfile.TemporaryFile(mode="w+") as log:
        server = subprocess.Popen([str(BIN / "server")], cwd=ROOT, env=ENV, stdout=log, stderr=log)
        try:
            wait_for(lambda: http("/readyz", expected=204))
            http("/healthz", token="", expected=204)
            http("/reservations", {}, token="invalid", expected=401)
            reservation_id = str(uuid.uuid4())
            body = {"id": reservation_id, "sku": "BOOK-001", "quantity": 3, "unit_price_yen": 1200}
            reservation, headers = http("/reservations", body, headers={"traceparent": f"00-{trace_id}-0123456789abcdef-01"})
            assert headers["x-trace-id"] == trace_id
            assert headers["x-request-id"]
            replay, _ = http("/reservations", body)
            assert replay == reservation
            http("/reservations", body | {"quantity": 0}, expected=422)
            http("/reservations", body | {"quantity": 4}, expected=409)
            with concurrent.futures.ThreadPoolExecutor(max_workers=8) as workers:
                orders = list(workers.map(lambda _: http(f"/reservations/{reservation_id}/confirm", method="POST")[0], range(8)))
            assert all(order == orders[0] for order in orders)
            order = orders[0]
            assert order["total_yen"] == 3600
            assert int(sql(f"SELECT COUNT(*) FROM orders WHERE reservation_id='{reservation_id}'")) == 1
            first, _ = http(f'/orders/{order["id"]}')
            second, _ = http(f'/orders/{order["id"]}')
            assert first == second == order
            cache_key = f'bbt:orders:v1:{order["id"]}'
            cached = json.loads(run("docker", "compose", "exec", "-T", "valkey", "valkey-cli", "GET", cache_key, capture_output=True).stdout)
            assert cached["id"] == order["id"]
            # L1 expires after ten seconds; the next lookup exercises Valkey (L2).
            time.sleep(11)
            assert http(f'/orders/{order["id"]}')[0] == order
            fallback_id = str(uuid.uuid4())
            http("/reservations", body | {"id": fallback_id})
            fallback_order = http(f"/reservations/{fallback_id}/confirm", method="POST")[0]
            run("docker", "compose", "stop", "valkey", stdout=subprocess.DEVNULL)
            try:
                assert http(f'/orders/{fallback_order["id"]}')[0] == fallback_order
            finally:
                run("docker", "compose", "start", "--wait", "valkey", stdout=subprocess.DEVNULL)

            sql(f"UPDATE reservations SET expires_at='2000-01-01T00:00:00Z' WHERE id='{reservation_id}'")
            expired_id = str(uuid.uuid4())
            http("/reservations", body | {"id": expired_id})
            sql(f"UPDATE reservations SET expires_at='2000-01-01T00:00:00Z' WHERE id='{expired_id}'")
            http(f"/reservations/{expired_id}/confirm", method="POST", expected=409)
            before = int(sql("SELECT COUNT(*) FROM reservations WHERE NOT confirmed AND expires_at <= clock_timestamp()"))
            output = run(str(BIN / "cleanup"), "--limit", "1", capture_output=True).stdout
            after = int(sql("SELECT COUNT(*) FROM reservations WHERE NOT confirmed AND expires_at <= clock_timestamp()"))
            assert before - after == 1 and "deleted=1" in output
            assert http(f"/reservations/{reservation_id}/confirm", method="POST")[0] == order

            # Historical fixture: simulate already-confirmed orders from a complete UTC day.
            day = "2000-01-02"
            sql(f"UPDATE orders SET confirmed_at='{day}T12:00:00Z' WHERE id='{order['id']}'")
            expected = sql(f"SELECT COUNT(*) || ',' || COALESCE(SUM(total_yen),0) FROM orders WHERE confirmed_at >= '{day}T00:00:00Z' AND confirmed_at < '2000-01-03T00:00:00Z'")
            batch_args = [str(BIN / "rebuild-sales"), "--from", day, "--until", "2000-01-03"]
            run(*batch_args, stdout=subprocess.DEVNULL)
            report = http(f"/sales/{day}")[0]
            run(*batch_args, stdout=subprocess.DEVNULL)
            assert http(f"/sales/{day}")[0] == report
            assert f'{report["order_count"]},{report["total_yen"]}' == expected
            # An empty day still publishes a zero total, allowing consumers to distinguish
            # a completed empty day from a report that has never been computed.
            run(str(BIN / "rebuild-sales"), "--from", "1999-01-01", "--until", "1999-01-02", stdout=subprocess.DEVNULL)
            assert http("/sales/1999-01-01")[0]["total_yen"] == 0
        except BaseException:
            server.terminate()
            server.wait(timeout=20)
            log.seek(0)
            print(log.read()[-16000:])
            raise
        finally:
            if server.poll() is None:
                server.terminate()
                server.wait(timeout=20)
        assert server.returncode == 0, server.returncode
        log.seek(0)
        logs = log.read()
        assert '"cache.tier":"memory"' in logs and '"cache.tier":"valkey"' in logs, logs[-4000:]

    def exported():
        with urllib.request.urlopen(f"http://127.0.0.1:16686/api/traces/{trace_id}") as response:
            traces = json.load(response)["data"]
        if not traces:
            return False
        spans = traces[0]["spans"]
        roots = [s for s in spans if s["operationName"] == "http.request"]
        if len(spans) < 3 or not roots:
            return False
        root = roots[0]
        assert any(ref["spanID"] == "0123456789abcdef" for ref in root["references"])
        assert any(ref["spanID"] == root["spanID"] for span in spans for ref in span["references"])
        return True
    wait_for(exported)
    print("PASS: authentication, validation, concurrent confirmation, L1/L2 cache, cache outage, bounded cleanup, replayable reports, graceful shutdown, OTLP trace propagation")


if __name__ == "__main__":
    main()
