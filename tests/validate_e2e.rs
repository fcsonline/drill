use std::process::Command;

mod common;

fn fixture(name: &str) -> String {
  format!("tests/fixtures/validate/{name}")
}

fn run_validate(args: &[&str]) -> std::process::Output {
  Command::new(common::drill_bin()).arg("validate").args(args).output().expect("drill validate should run")
}

fn stdout_starts_with_json(output: &std::process::Output) -> bool {
  let s = String::from_utf8_lossy(&output.stdout);
  s.trim_start().starts_with('[')
}

/// A valid benchmark exits 0 (clean).
#[test]
fn valid_exits_zero() {
  let out = run_validate(&[&fixture("valid.yml")]);
  assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
  assert!(String::from_utf8_lossy(&out.stdout).contains("OK"));
}

/// Errors produce exit code 1.
#[test]
fn error_heavy_exits_one() {
  let out = run_validate(&[&fixture("error-heavy.yml")]);
  assert_eq!(out.status.code(), Some(1));
  assert!(String::from_utf8_lossy(&out.stdout).contains("error"));
}

/// Warnings and suggestions alone do not fail (exit 0).
#[test]
fn warning_heavy_exits_zero() {
  let out = run_validate(&[&fixture("warning-heavy.yml")]);
  assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
  let s = String::from_utf8_lossy(&out.stdout);
  assert!(s.contains("warning"));
}

/// Suggestions alone do not fail (exit 0).
#[test]
fn suggestion_heavy_exits_zero() {
  let out = run_validate(&[&fixture("suggestion-heavy.yml")]);
  assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
  assert!(String::from_utf8_lossy(&out.stdout).contains("suggestion"));
}

/// Malformed YAML → exit 1.
#[test]
fn malformed_exits_one() {
  let out = run_validate(&[&fixture("malformed.yml")]);
  assert_eq!(out.status.code(), Some(1));
}

/// Recursive include of a valid sub-plan validates clean.
#[test]
fn include_resolves_cleanly() {
  let out = run_validate(&[&fixture("include-main.yml")]);
  assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// Include cycle → exit 1.
#[test]
fn include_cycle_exits_one() {
  let out = run_validate(&[&fixture("include-cycle.yml")]);
  assert_eq!(out.status.code(), Some(1));
}

/// `--format json` emits a parseable JSON array on stdout (even for errors).
#[test]
fn json_format_is_returned_on_errors() {
  let out = run_validate(&["--format", "json", &fixture("error-heavy.yml")]);
  assert_eq!(out.status.code(), Some(1));
  assert!(stdout_starts_with_json(&out));

  let stdout = String::from_utf8_lossy(&out.stdout);
  let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("stdout should be strict JSON");
  let arr = v.as_array().expect("JSON should be an array");
  assert!(!arr.is_empty());
  for entry in arr {
    let obj = entry.as_object().expect("each entry is an object");
    assert!(obj.contains_key("severity"));
    assert!(obj.contains_key("location"));
    assert!(obj.contains_key("message"));
  }
}

/// Clean file in JSON mode returns `[]` and exit 0.
#[test]
fn json_format_empty_for_clean() {
  let out = run_validate(&["--format", "json", &fixture("valid.yml")]);
  assert!(out.status.success());
  let stdout = String::from_utf8_lossy(&out.stdout);
  let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
  assert_eq!(v.as_array().unwrap().len(), 0);
}
