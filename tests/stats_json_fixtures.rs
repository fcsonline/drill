//! Guard tests for the published NDJSON fixture set (`tests/fixtures/stats-json/`).
//!
//! The RFP (`docs/drill-results-stream-rfp.md`, §9) commits Drill to ship a
//! static fixture corpus that downstream consumers (e.g. Sutomu's
//! `parse_drill_stream`) can validate their NDJSON parsers against, independent
//! of running Drill. This suite re-checks that the committed fixture files
//! still match the documented schema (docs/stats-json.md), so a consumer never
//! builds against a fixture set that no longer reflects the real stream shape.
//!
//! Unlike `tests/stats_json_e2e.rs` (which runs the binary against a mock
//! server), every test here reads only the committed files — no network, no
//! spawned process — so they are a cheap, deterministic contract guard.

/// Reads `tests/fixtures/stats-json/<name>` lines, skipping blank lines.
fn read_fixture_lines(name: &str) -> Vec<String> {
  std::fs::read_to_string(format!("tests/fixtures/stats-json/{name}")).unwrap_or_else(|e| panic!("missing fixture {name}: {e}")).lines().filter(|l| !l.trim().is_empty()).map(|l| l.to_string()).collect()
}

/// Strict-parse a single line as JSON; panics with the offending line.
fn parse_line(line: &str) -> serde_json::Value {
  serde_json::from_str(line).unwrap_or_else(|e| panic!("fixture line is not strict JSON ({e}): {line:?}"))
}

#[test]
fn basic_covers_intervals_and_final_record() {
  let lines = read_fixture_lines("basic.ndjson");
  // RFP §9: 2 endpoints, 3 intervals, final record.
  assert_eq!(lines.len(), 4, "expected 3 interval records + 1 final record");
  let records: Vec<serde_json::Value> = lines.iter().map(|l| parse_line(l)).collect();

  for bytes in &records[..3] {
    assert_eq!(bytes["version"], 1);
    assert!(bytes.get("interval").is_some(), "interval record carries `interval`");
    assert!(bytes.get("final").is_none(), "interval record must not carry `final`");
    assert!(bytes["endpoints"].is_array());
    assert_eq!(bytes["endpoints"][0]["name"], "Get root");
    // Per-endpoint throughput (RFP §4.2): rps + failures_per_sec present.
    for endpoint in bytes["endpoints"].as_array().unwrap() {
      assert!(endpoint["rps"].is_f64());
      assert!(endpoint["failures_per_sec"].is_f64());
    }
    // Each interval carries a `global` aggregate object (RFP §4.3).
    assert!(bytes["global"].is_object());
    assert!(bytes["global"]["rps"].is_f64());
    assert!(bytes["global"]["failures_per_sec"].is_f64());
    assert!(bytes["global"].get("status").is_none(), "interval record carries no `status`");
  }

  let final_record = records.last().unwrap();
  assert_eq!(final_record["version"], 1);
  assert_eq!(final_record["final"], true);
  assert_eq!(final_record["global"]["status"], "completed");
  assert!(final_record["global"]["duration_sec"].is_f64());
  assert!(final_record["global"]["time_elapsed_sec"].is_f64());
  assert!(final_record["global"]["rps"].is_f64());
  assert!(final_record["global"]["failures_per_sec"].is_f64());
}

#[test]
fn empty_is_single_zeroed_final_record() {
  let lines = read_fixture_lines("empty.ndjson");
  assert_eq!(lines.len(), 1, "empty run emits exactly one final record");
  let record = parse_line(&lines[0]);

  assert_eq!(record["version"], 1);
  assert_eq!(record["final"], true);
  assert_eq!(record["endpoints"].as_array().unwrap().len(), 0);
  let global = &record["global"];
  assert_eq!(global["total_requests"], 0);
  assert_eq!(global["successful_requests"], 0);
  assert_eq!(global["failed_requests"], 0);
  assert_eq!(global["rps"], 0.0);
  assert_eq!(global["status"], "failed", "empty plan renders status=failed per docs/stats-json.md");
}

#[test]
fn assert_fail_has_failed_status_and_failures_per_sec() {
  let lines = read_fixture_lines("assert-fail.ndjson");
  let final_record = parse_line(lines.last().unwrap());

  assert_eq!(final_record["version"], 1);
  assert_eq!(final_record["final"], true);
  assert_eq!(final_record["global"]["status"], "failed");
  assert!(final_record["global"]["failures_per_sec"].is_f64());
  let endpoint = &final_record["endpoints"][0];
  assert_eq!(endpoint["name"], "Get root");
  assert!(endpoint["failed_requests"].as_u64().unwrap() >= 1, "assert-fail fixture reflects a failure");
}

#[test]
fn early_cancel_has_cancelled_status_and_partial_counters() {
  let lines = read_fixture_lines("early-cancel.ndjson");
  let records: Vec<serde_json::Value> = lines.iter().map(|l| parse_line(l)).collect();

  // Intervals precede the terminal cancelled record.
  assert!(records.len() >= 3, "early-cancel shows a couple of intervals + final");
  for bytes in &records[..records.len() - 1] {
    assert_eq!(bytes["version"], 1);
    assert!(bytes.get("interval").is_some());
  }

  let final_record = records.last().unwrap();
  assert_eq!(final_record["final"], true);
  assert_eq!(final_record["global"]["status"], "cancelled");
  // Partial counters: fewer than the full requested run.
  assert!(final_record["global"]["total_requests"].as_u64().unwrap() < 200, "cancelled run reports partial counters");
}

#[test]
fn malformed_allows_skipping_a_corrupt_line() {
  let lines = read_fixture_lines("malformed.ndjson");
  // The fixture intentionally embeds a non-JSON line the consumer must skip
  // without aborting (RFP §10). Only JSON lines are parsed here.
  let parsed: Vec<serde_json::Value> = lines.iter().filter(|l| serde_json::from_str::<serde_json::Value>(l).is_ok()).map(|l| parse_line(l)).collect();

  assert!(lines.len() > parsed.len(), "malformed fixture must contain at least one corrupt line");
  assert!(!parsed.is_empty(), "consumer must still obtain records after skipping the corrupt line");

  // The stream is still terminated by a valid final record.
  let final_record = parsed.last().unwrap();
  assert_eq!(final_record["final"], true);
  assert_eq!(final_record["global"]["status"], "completed");
}
