use std::fs;
use std::path::PathBuf;
use std::process::Command;

use wiremock::{
  Mock, MockServer, ResponseTemplate,
  matchers::{method, path},
};

mod common;

fn drill_bin() -> PathBuf {
  common::drill_bin()
}

fn write_benchmark(base: &str, iterations: i64, body: &str) -> (PathBuf, tempfile::TempDir) {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("benchmark.yml");
  fs::write(&path, format!("---\nconcurrency: 1\nbase: '{base}'\niterations: {iterations}\n\nplan:\n{body}\n", base = base, iterations = iterations, body = body)).unwrap();
  (path, dir)
}

/// Strips ANSI color escapes and masks digit runs (timings, counts, ports are
/// volatile across runs) so the fixed text frame of the human `--stats` table
/// can be compared deterministically.
fn normalize_frame(raw: &[u8]) -> String {
  let text = String::from_utf8_lossy(raw);
  let no_ansi = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap().replace_all(&text, "");
  let masked = regex::Regex::new(r"\d+").unwrap().replace_all(&no_ansi, "#");
  // Collapse trailing whitespace per line so column padding differences do not
  // trip the comparison.
  masked.lines().map(|l| l.trim_end().to_string()).collect::<Vec<_>>().join("\n") + "\n"
}

/// Acceptance #7: the human `--stats` console table stays byte-identical
/// (modulo volatile timings/counts/ports) to the committed frame. Guards the
/// labels, ordering, and structure of the legacy path from accidental changes
/// while `--stats-json` ships.
#[tokio::test]
async fn stats_human_output_frame_is_stable() {
  let server = MockServer::start().await;
  Mock::given(method("GET")).and(path("/")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "ok"}))).mount(&server).await;

  let (benchmark, _dir) = write_benchmark(
    &server.uri(),
    2,
    r#"  - name: Get root
    request:
      url: /
"#,
  );

  let output = Command::new(drill_bin()).arg("--benchmark").arg(&benchmark).arg("--stats").output().expect("drill should run");

  assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

  let golden_path = PathBuf::from("tests/fixtures/stats-human/stats-frame.golden.txt");
  let actual = normalize_frame(&output.stdout);

  if std::env::var_os("UPDATE_GOLDEN").is_some() {
    fs::create_dir_all(golden_path.parent().unwrap()).unwrap();
    fs::write(&golden_path, &actual).unwrap();
    return;
  }

  let golden = fs::read_to_string(&golden_path).unwrap_or_else(|_| panic!("missing golden {} — run once with UPDATE_GOLDEN=1 after verifying the --stats frame is correct", golden_path.display()));
  assert_eq!(actual, golden, "human --stats frame drifted; run with UPDATE_GOLDEN=1 to refresh");
}
