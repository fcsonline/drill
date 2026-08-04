use std::fs;
use std::path::PathBuf;

use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};

/// A simple mock API server fixture for drill E2E tests.
pub struct MockApi {
  pub server: MockServer,
}

impl MockApi {
  /// Starts a new mock server and mounts a default set of endpoints.
  pub async fn start() -> Self {
    let server = MockServer::start().await;

    Mock::given(method("GET")).and(path("/")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "ok"}))).mount(&server).await;

    Mock::given(method("POST")).and(path("/echo")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"received": "{{ body }}"}))).mount(&server).await;

    Mock::given(method("GET")).and(path("/delay")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "delayed"}))).mount(&server).await;

    Mock::given(method("GET")).and(path("/error")).respond_with(ResponseTemplate::new(500).set_body_string("server error")).mount(&server).await;

    Mock::given(method("GET")).and(path("/notfound")).respond_with(ResponseTemplate::new(404).set_body_string("not found")).mount(&server).await;

    MockApi {
      server,
    }
  }

  /// Returns the base URL for the mock server.
  pub fn base_url(&self) -> String {
    self.server.uri()
  }

  /// Writes a benchmark YAML file to a temp directory and returns its path.
  pub fn write_benchmark(&self, iterations: i64, results_dir: &str, body: &str) -> PathBuf {
    let path = PathBuf::from(results_dir).join("benchmark.yml");
    fs::create_dir_all(results_dir).unwrap();
    fs::write(
      &path,
      format!(
        r#"---
concurrency: 1
base: '{base}'
iterations: {iterations}

results:
  output_dir: {results_dir}
  csv: true
  html: true

plan:
{body}
"#,
        base = self.base_url(),
        iterations = iterations,
        results_dir = results_dir,
        body = body
      ),
    )
    .unwrap();
    path
  }
}

/// Returns the path to the compiled drill binary.
pub fn drill_bin() -> PathBuf {
  std::env::var("CARGO_BIN_EXE_drill").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("target/debug/drill"))
}
