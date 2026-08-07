use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;
use serde_json::json;
use wiremock::{
  Mock, MockServer, ResponseTemplate,
  matchers::{method, path},
};

mod common;

fn drill_bin() -> PathBuf {
  common::drill_bin()
}

/// Starts a mock server and mounts a slow endpoint so a run spans several
/// intervals, plus the standard fast endpoints.
async fn start_mock_with_slow_endpoint() -> MockServer {
  let server = MockServer::start().await;
  Mock::given(method("GET")).and(path("/")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "ok"}))).mount(&server).await;
  Mock::given(method("GET")).and(path("/slow")).respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_millis(300))).mount(&server).await;
  server
}

/// Writes a benchmark YAML with the given plan body (no `results` block so
/// stdout carries only the NDJSON stream).
/// Writes a benchmark YAML with the given plan body (no `results` block so
/// stdout carries only the NDJSON stream). Owns the temp dir so the file stays
/// alive for the test's lifetime.
struct BenchmarkYaml {
  path: PathBuf,
  _dir: tempfile::TempDir,
}

fn write_benchmark(base: &str, iterations: i64, body: &str) -> BenchmarkYaml {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("benchmark.yml");
  fs::write(&path, format!("---\nconcurrency: 1\nbase: '{base}'\niterations: {iterations}\n\nplan:\n{body}\n", base = base, iterations = iterations, body = body)).unwrap();
  BenchmarkYaml {
    path,
    _dir: dir,
  }
}

/// Strict NDJSON parse: every non-empty stdout line must be a JSON object.
fn parse_stream(output: &Output) -> Vec<Value> {
  let stdout = String::from_utf8_lossy(&output.stdout);
  let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
  assert!(!lines.is_empty(), "expected NDJSON on stdout, got: {stdout:?}");
  lines.iter().map(|l| serde_json::from_str::<Value>(l).unwrap_or_else(|e| panic!("line is not strict JSON ({e}): {l:?}"))).collect()
}

/// Acceptance #1: one NDJSON line per interval plus a terminal `final: true`
/// line, all strict-parseable. Acceptance #2: version on every line; final
/// line has `final: true` and global.status/rps/failures_per_sec/duration_sec.
#[tokio::test]
async fn emits_strict_ndjson_with_interval_and_final_records() {
  let server = start_mock_with_slow_endpoint().await;
  let benchmark = write_benchmark(
    &server.uri(),
    6,
    r#"  - name: Get root
    request:
      url: /slow
"#,
  );

  let output = Command::new(drill_bin()).arg("--benchmark").arg(&benchmark.path).arg("--stats-json").arg("--stats-interval").arg("1").output().expect("drill should run");

  assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
  let records = parse_stream(&output);

  assert!(records.len() >= 2, "expected >= 1 interval record + final record, got {}", records.len());
  for record in &records[..records.len() - 1] {
    assert_eq!(record["version"], json!(1), "interval record carries version");
    assert!(record.get("interval").is_some(), "interval record carries interval number");
    assert!(record["global"].is_object());
    assert!(record["global"]["rps"].is_f64());
    assert!(record["global"]["failures_per_sec"].is_f64());
    assert!(record["global"].get("status").is_none(), "interval records carry no status");
  }

  let final_record = records.last().unwrap();
  assert_eq!(final_record["version"], json!(1));
  assert_eq!(final_record["final"], json!(true));
  let global = &final_record["global"];
  assert_eq!(global["status"], json!("completed"));
  assert!(global["rps"].is_f64());
  assert!(global["failures_per_sec"].is_f64());
  assert!(global["duration_sec"].is_f64());
  assert!(global["total_requests"].as_u64().unwrap_or(0) >= 1, "final record reflects executed requests");
}

/// Acceptance #3: each interval line carries per-endpoint `rps` and
/// `failures_per_sec`.
#[tokio::test]
async fn interval_records_carry_per_endpoint_rps_and_failures_per_sec() {
  let server = start_mock_with_slow_endpoint().await;
  let benchmark = write_benchmark(
    &server.uri(),
    4,
    r#"  - name: Get root
    request:
      url: /slow

  - name: Get missing
    request:
      url: /missing
"#,
  );

  let output = Command::new(drill_bin()).arg("--benchmark").arg(&benchmark.path).arg("--stats-json").arg("--stats-interval").arg("1").output().expect("drill should run");

  assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
  let records = parse_stream(&output);

  for record in &records[..records.len() - 1] {
    let endpoints = record["endpoints"].as_array().unwrap();
    for endpoint in endpoints {
      assert!(endpoint["rps"].is_f64(), "endpoint {endpoint} carries rps");
      assert!(endpoint["failures_per_sec"].is_f64(), "endpoint {endpoint} carries failures_per_sec");
      assert!(endpoint["name"].is_string());
    }
  }
}

/// Acceptance #4: `--stats-interval` without `--stats-json` exits non-zero.
#[tokio::test]
async fn stats_interval_without_stats_json_is_rejected() {
  let server = MockServer::start().await;
  let benchmark = write_benchmark(&server.uri(), 1, "  - name: Get root\n    request:\n      url: /\n");

  let output = Command::new(drill_bin()).arg("--benchmark").arg(&benchmark.path).arg("--stats-interval").arg("2").output().expect("drill should run");

  assert!(!output.status.success(), "--stats-interval must be rejected without --stats-json");
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("stats-json"), "error should mention --stats-json, got: {stderr}");
}

/// Acceptance #4: `--stats-interval 0` is rejected.
#[tokio::test]
async fn stats_interval_zero_is_rejected() {
  let server = MockServer::start().await;
  let benchmark = write_benchmark(&server.uri(), 1, "  - name: Get root\n    request:\n      url: /\n");

  let output = Command::new(drill_bin()).arg("--benchmark").arg(&benchmark.path).arg("--stats-json").arg("--stats-interval").arg("0").output().expect("drill should run");

  assert!(!output.status.success(), "--stats-interval 0 must be rejected");
}

/// Acceptance #5: a run with zero requests still emits a final record.
#[tokio::test]
async fn empty_benchmark_emits_final_record() {
  let server = MockServer::start().await;
  // An explicit empty sequence (`plan: []`) — a bare `plan:` parses as a
  // missing node and the reader rejects it before reaching the run loop.
  let benchmark = write_benchmark(&server.uri(), 1, "  []");

  let output = Command::new(drill_bin()).arg("--benchmark").arg(&benchmark.path).arg("--stats-json").output().expect("drill should run");

  assert_eq!(output.status.code(), Some(1), "empty benchmark exits 1");
  let records = parse_stream(&output);
  assert_eq!(records.len(), 1, "empty run emits exactly one final record");
  let final_record = &records[0];
  assert_eq!(final_record["final"], json!(true));
  assert_eq!(final_record["global"]["status"], json!("failed"));
  assert_eq!(final_record["global"]["total_requests"], json!(0), "zeroed counters");
}

/// Acceptance #6: SIGTERM mid-run emits a partial final record with
/// `status: "cancelled"` before exit.
#[cfg(unix)]
#[tokio::test]
async fn sigterm_emits_cancelled_final_record() {
  use std::time::Duration;

  let server = start_mock_with_slow_endpoint().await;
  let benchmark = write_benchmark(
    &server.uri(),
    100,
    r#"  - name: Get root
    request:
      url: /slow
"#,
  );

  let child = Command::new(drill_bin()).arg("--benchmark").arg(&benchmark.path).arg("--stats-json").arg("--stats-interval").arg("1").stdout(std::process::Stdio::piped()).spawn().expect("drill should spawn");

  tokio::time::sleep(Duration::from_millis(1800)).await;
  let status = Command::new("kill").arg("-TERM").arg(child.id().to_string()).status().expect("kill should run");
  assert!(status.success(), "kill -TERM should succeed");

  let output = child.wait_with_output().expect("drill should exit");
  assert!(output.status.success(), "graceful SIGTERM shutdown exits 0 (exit code reflects assertion failures only); stderr: {}", String::from_utf8_lossy(&output.stderr));
  let records = parse_stream(&output);

  let final_record = records.last().unwrap();
  assert_eq!(final_record["final"], json!(true));
  assert_eq!(final_record["global"]["status"], json!("cancelled"));
  let total = final_record["global"]["total_requests"].as_u64().unwrap();
  assert!(total < 100, "cancelled run reports partial counters, got {total}");
}

/// Acceptance #8 + stdout purity: the NDJSON stream is line-buffered and
/// stdout carries only parseable NDJSON lines (diagnostics go to stderr).
#[tokio::test]
async fn stdout_carries_only_ndjson_lines() {
  let server = MockServer::start().await;
  let benchmark = write_benchmark(
    &server.uri(),
    2,
    r#"  - name: Get root
    request:
      url: /
"#,
  );

  let output = Command::new(drill_bin()).arg("--benchmark").arg(&benchmark.path).arg("--stats-json").arg("--verbose").output().expect("drill should run");

  assert!(output.status.success());
  let stdout = String::from_utf8_lossy(&output.stdout);
  for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
    serde_json::from_str::<Value>(line).unwrap_or_else(|e| panic!("stdout line is not NDJSON ({e}): {line:?}"));
  }
  assert!(stdout.ends_with('\n'), "stream is line-buffered: each record ends with a newline");
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(!stderr.is_empty(), "verbose diagnostics go to stderr");
}
