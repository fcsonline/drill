use std::process::Command;

use common::MockApi;

mod common;

/// Verifies that drill captures and reports curl-style metrics for a simple GET request.
#[tokio::test]
async fn reports_metrics_for_get_request() {
  let api = MockApi::start().await;
  let results_dir = tempfile::tempdir().unwrap();
  let benchmark = api.write_benchmark(
    2,
    results_dir.path().to_str().unwrap(),
    r#"  - name: Get root
    request:
      url: /
"#,
  );

  let output = Command::new(common::drill_bin()).arg("--benchmark").arg(benchmark).arg("--stats").output().expect("drill binary should run");

  assert!(output.status.success(), "drill failed: {}", String::from_utf8_lossy(&output.stderr));

  let csv = results_dir.path().join("stats.csv");
  let csv_content = std::fs::read_to_string(&csv).expect("stats.csv should be generated");
  assert!(csv_content.contains("Min TTFB (ms)"), "CSV should contain TTFB column");
  assert!(csv_content.contains("Total Download (bytes)"), "CSV should contain download size column");
  assert!(csv_content.contains("Get root"), "CSV should contain the request name");

  // The GET response body is non-empty. With two iterations, total download should be > 0.
  let line = csv_content.lines().find(|l| l.starts_with("Get root")).expect("Get root row should exist");
  let total_download: u64 = line.split(',').nth(26).expect("total download column should exist").parse().expect("total download should be a number");
  assert!(total_download > 0, "total download should be greater than 0");
}

/// Verifies that drill captures upload size for a POST request with a body.
#[tokio::test]
async fn reports_upload_size_for_post_request() {
  let api = MockApi::start().await;
  let results_dir = tempfile::tempdir().unwrap();
  let benchmark = api.write_benchmark(
    1,
    results_dir.path().to_str().unwrap(),
    r#"  - name: Echo POST
    request:
      url: /echo
      method: POST
      body: hello=world
      headers:
        Content-Type: 'application/x-www-form-urlencoded'
"#,
  );

  let output = Command::new(common::drill_bin()).arg("--benchmark").arg(benchmark).arg("--stats").output().expect("drill binary should run");

  assert!(output.status.success(), "drill failed: {}", String::from_utf8_lossy(&output.stderr));

  let csv = results_dir.path().join("stats.csv");
  let csv_content = std::fs::read_to_string(&csv).expect("stats.csv should be generated");
  assert!(csv_content.contains("Total Upload (bytes)"), "CSV should contain upload size column");
  // The POST body is 11 bytes (hello=world). With one iteration, total upload is 11.
  assert!(csv_content.contains(",11,"), "CSV should contain total upload of 11 bytes");
}

/// Verifies that metrics are produced even when the server returns an error.
#[tokio::test]
async fn reports_metrics_for_error_response() {
  let api = MockApi::start().await;
  let results_dir = tempfile::tempdir().unwrap();
  let benchmark = api.write_benchmark(
    1,
    results_dir.path().to_str().unwrap(),
    r#"  - name: Error request
    request:
      url: /error
"#,
  );

  let output = Command::new(common::drill_bin()).arg("--benchmark").arg(benchmark).arg("--stats").output().expect("drill binary should run");

  assert!(output.status.success(), "drill failed: {}", String::from_utf8_lossy(&output.stderr));

  let csv = results_dir.path().join("stats.csv");
  let csv_content = std::fs::read_to_string(&csv).expect("stats.csv should be generated");
  assert!(csv_content.contains("Error request"), "CSV should contain the error request name");
  assert!(csv_content.contains(",1,"), "CSV should report one failure");
}

/// Verifies that the HTML report also contains the new metrics columns.
#[tokio::test]
async fn reports_metrics_in_html() {
  let api = MockApi::start().await;
  let results_dir = tempfile::tempdir().unwrap();
  let benchmark = api.write_benchmark(
    1,
    results_dir.path().to_str().unwrap(),
    r#"  - name: Get root
    request:
      url: /
"#,
  );

  let output = Command::new(common::drill_bin()).arg("--benchmark").arg(benchmark).output().expect("drill binary should run");

  assert!(output.status.success(), "drill failed: {}", String::from_utf8_lossy(&output.stderr));

  let html = results_dir.path().join("report.html");
  let html_content = std::fs::read_to_string(&html).expect("report.html should be generated");
  assert!(html_content.contains("Min TTFB (ms)"), "HTML should contain TTFB column");
  assert!(html_content.contains("Avg Download (bytes)"), "HTML should contain download size column");
  assert!(html_content.contains("Avg Size (bytes)"), "HTML should contain total size column");
}
