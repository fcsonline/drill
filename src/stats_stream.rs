//! Live NDJSON result stream (`--stats-json`) per the RFP in
//! `docs/drill-results-stream-rfp.md`.
//!
//! Emission happens inside the benchmark run, not after it: a wall-clock
//! ticker task drains the shared report store every `--stats-interval`
//! seconds and writes one interval record per tick, flushed per line. The
//! terminal record is produced by [`StatsStream::finalize`] with a machine
//! readable `status`, and is always emitted before the run returns so every
//! exit path in `main.rs` is preceded by it.

use std::io::{self, BufWriter, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use linked_hash_map::LinkedHashMap;
use serde_json::{Value, json};

use crate::actions::Report;
use crate::results;

/// Schema version carried by every record (`version` field, RFP §4.1).
pub const SCHEMA_VERSION: u64 = 1;

/// Terminal state carried by the final record's `global.status` (RFP §6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalStatus {
  Completed,
  Failed,
  Cancelled,
}

impl FinalStatus {
  pub fn as_str(&self) -> &'static str {
    match self {
      FinalStatus::Completed => "completed",
      FinalStatus::Failed => "failed",
      FinalStatus::Cancelled => "cancelled",
    }
  }
}

/// Streams interval records from a shared report store and emits the terminal
/// record on [`finalize`](Self::finalize).
pub struct StatsStream {
  store: Arc<Mutex<Vec<Report>>>,
  success_codes: Arc<[u16]>,
  begin: tokio::time::Instant,
  done: Arc<AtomicBool>,
  notify: Arc<tokio::sync::Notify>,
  handle: Option<tokio::task::JoinHandle<()>>,
}

impl std::fmt::Debug for StatsStream {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("StatsStream").field("success_codes", &self.success_codes).field("done", &self.done).finish_non_exhaustive()
  }
}

impl StatsStream {
  /// Spawns the interval ticker task over `store`. The run loop pushes
  /// completed iteration reports into `store`; each wall-clock tick emits one
  /// NDJSON interval record (zeroed when the slice is empty).
  pub fn start(interval_secs: u64, store: Arc<Mutex<Vec<Report>>>, success_codes: Arc<[u16]>) -> Self {
    let begin = tokio::time::Instant::now();
    let done = Arc::new(AtomicBool::new(false));
    let notify = Arc::new(tokio::sync::Notify::new());

    let handle = {
      let store = store.clone();
      let success_codes = success_codes.clone();
      let done = done.clone();
      let notify = notify.clone();
      tokio::spawn(async move {
        // The CLI enforces `--stats-interval >= 1` at parse time; the floor here
        // is a defensive guard so a programmatic/zero interval cannot make the
        // ticker busy-spin the shared store.
        let period = Duration::from_secs(interval_secs.max(1));
        let mut interval = tokio::time::interval_at(begin + period, period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut watermark = 0usize;
        let mut last_tick = begin;
        let mut interval_no: u64 = 0;

        loop {
          tokio::select! {
            _ = interval.tick() => {}
            _ = notify.notified() => {
              if done.load(Ordering::SeqCst) {
                break;
              }
              continue;
            }
          }
          if done.load(Ordering::SeqCst) {
            break;
          }

          let now = tokio::time::Instant::now();
          let slice_duration = now.duration_since(last_tick).as_secs_f64();
          last_tick = now;

          let slice: Vec<Report> = {
            let guard = store.lock().unwrap();
            let new = guard[watermark..].to_vec();
            watermark = guard.len();
            new
          };

          interval_no += 1;
          let line = build_interval_record(&slice, &success_codes, interval_no, slice_duration, begin.elapsed().as_secs_f64());
          let out = io::stdout();
          let mut w = BufWriter::new(out.lock());
          // A failed write means the consumer is gone (e.g. `| head` closed
          // the pipe): stop ticking instead of silently dropping every record.
          if writeln!(w, "{line}").and_then(|_| w.flush()).is_err() {
            break;
          }
        }
      })
    };

    StatsStream {
      store,
      success_codes,
      begin,
      done,
      notify,
      handle: Some(handle),
    }
  }

  /// Stops the ticker, drains the remaining reports, and writes the terminal
  /// record with `status`. The final record is always emitted, even for a
  /// zero-request run (zeroed counters, RFP §4.4/§10).
  pub async fn finalize(mut self, status: FinalStatus) -> io::Result<()> {
    self.done.store(true, Ordering::SeqCst);
    self.notify.notify_one();
    if let Some(handle) = self.handle.take()
      && handle.await.is_err()
    {
      eprintln!("Warning: stats stream ticker task panicked");
    }

    let all_reports: Vec<Report> = {
      let mut guard = self.store.lock().unwrap();
      std::mem::take(&mut *guard)
    };
    let duration = self.begin.elapsed().as_secs_f64();
    let line = build_final_record(&all_reports, &self.success_codes, duration, status);

    let out = io::stdout();
    let mut w = BufWriter::new(out.lock());
    writeln!(w, "{line}")?;
    w.flush()
  }
}

/// Per-endpoint JSON object (§4.2) including `rps` and `failures_per_sec`.
fn endpoint_object(stats: &results::Stats) -> Value {
  json!({
    "name": stats.name,
    "total_requests": stats.total,
    "successful_requests": stats.total - stats.failure,
    "failed_requests": stats.failure,
    "avg_ms": stats.mean_ms,
    "median_ms": stats.median_ms,
    "stdev_ms": stats.stdev_ms,
    "p50_ms": stats.p50_ms,
    "p66_ms": stats.p66_ms,
    "p75_ms": stats.p75_ms,
    "p80_ms": stats.p80_ms,
    "p90_ms": stats.p90_ms,
    "p95_ms": stats.p95_ms,
    "p98_ms": stats.p98_ms,
    "p99_ms": stats.p99_ms,
    "p999_ms": stats.p999_ms,
    "p9999_ms": stats.p9999_ms,
    "max_ms": stats.max_ms,
    "rps": stats.rps,
    "failures_per_sec": stats.failures_per_sec,
  })
}

/// Global aggregate object (§4.4). `status` is only present on the final
/// record.
fn global_object(stats: &results::Stats, duration_sec: f64, time_elapsed_sec: f64, status: Option<FinalStatus>) -> Value {
  let mut global = json!({
    "total_requests": stats.total,
    "successful_requests": stats.total - stats.failure,
    "failed_requests": stats.failure,
    "avg_ms": stats.mean_ms,
    "median_ms": stats.median_ms,
    "stdev_ms": stats.stdev_ms,
    "p50_ms": stats.p50_ms,
    "p66_ms": stats.p66_ms,
    "p75_ms": stats.p75_ms,
    "p80_ms": stats.p80_ms,
    "p90_ms": stats.p90_ms,
    "p95_ms": stats.p95_ms,
    "p98_ms": stats.p98_ms,
    "p99_ms": stats.p99_ms,
    "p999_ms": stats.p999_ms,
    "p9999_ms": stats.p9999_ms,
    "max_ms": stats.max_ms,
    "rps": stats.rps,
    "failures_per_sec": stats.failures_per_sec,
    "duration_sec": duration_sec,
    "time_elapsed_sec": time_elapsed_sec,
  });
  if let Some(s) = status {
    global["status"] = json!(s.as_str());
  }
  global
}

/// Zeroed global for idle/empty slices (§1.5.3, §10): reuses the same field
/// list as [`global_object`] by passing a zeroed [`results::Stats`].
fn zeroed_global(duration_sec: f64, time_elapsed_sec: f64, status: Option<FinalStatus>) -> Value {
  global_object(&results::Stats::default(), duration_sec, time_elapsed_sec, status)
}

fn group_by_name(reports: &[Report]) -> LinkedHashMap<String, Vec<&Report>> {
  let mut by_name: LinkedHashMap<String, Vec<&Report>> = LinkedHashMap::new();
  for report in reports {
    by_name.entry(report.name.clone()).or_default().push(report);
  }
  by_name
}

fn build_interval_record(slice: &[Report], success_codes: &[u16], interval_no: u64, slice_duration: f64, time_elapsed_sec: f64) -> String {
  let duration = slice_duration.max(f64::EPSILON);

  let (endpoints, global) = if slice.is_empty() {
    (Value::Array(Vec::new()), zeroed_global(slice_duration, time_elapsed_sec, None))
  } else {
    let by_name = group_by_name(slice);
    let endpoint_items: Vec<Value> = by_name
      .iter()
      .map(|(name, reps)| {
        let s = results::compute_stats(name, reps.as_slice(), duration, success_codes);
        endpoint_object(&s)
      })
      .collect();

    let refs: Vec<&Report> = slice.iter().collect();
    let global = global_object(&results::compute_stats("Total", &refs, duration, success_codes), slice_duration, time_elapsed_sec, None);

    (Value::Array(endpoint_items), global)
  };

  serde_json::to_string(&json!({
    "version": SCHEMA_VERSION,
    "interval": interval_no,
    "time_elapsed_sec": time_elapsed_sec,
    "endpoints": endpoints,
    "global": global,
  }))
  .unwrap()
}

fn build_final_record(reports: &[Report], success_codes: &[u16], duration: f64, status: FinalStatus) -> String {
  let duration = duration.max(f64::EPSILON);

  let (endpoints, global) = if reports.is_empty() {
    (Value::Array(Vec::new()), zeroed_global(duration, duration, Some(status)))
  } else {
    let by_name = group_by_name(reports);
    let endpoint_items: Vec<Value> = by_name
      .iter()
      .map(|(name, reps)| {
        let s = results::compute_stats(name, reps.as_slice(), duration, success_codes);
        endpoint_object(&s)
      })
      .collect();

    let refs: Vec<&Report> = reports.iter().collect();
    let global = global_object(&results::compute_stats("Total", &refs, duration, success_codes), duration, duration, Some(status));

    (Value::Array(endpoint_items), global)
  };

  serde_json::to_string(&json!({
    "version": SCHEMA_VERSION,
    "final": true,
    "endpoints": endpoints,
    "global": global,
  }))
  .unwrap()
}

#[cfg(test)]
mod tests {
  use std::sync::Mutex;

  use super::*;

  fn report(name: &str, duration_ms: f64, status: u16, timestamp: f64) -> Report {
    Report {
      name: name.to_string(),
      duration: duration_ms,
      status,
      timestamp,
      metrics: crate::metrics::RequestMetrics {
        time_starttransfer_ms: 0.0,
        time_total_ms: duration_ms,
        size_request: 0,
        size_upload: 0,
        size_download: 0,
        size_header_request: 0,
        size_header_response: 0,
        size_total: 0,
        ..Default::default()
      },
    }
  }

  #[test]
  fn interval_record_has_version_and_global() {
    let slice = vec![report("Get root", 3.2, 200, 0.1), report("Get root", 4.1, 200, 0.5), report("Get other", 9.9, 500, 0.6)];
    let line = build_interval_record(&slice, &[], 1, 1.0, 1.0);
    let v: Value = serde_json::from_str(&line).unwrap();

    assert_eq!(v["version"], json!(SCHEMA_VERSION));
    assert_eq!(v["interval"], json!(1));
    assert!(v.get("final").is_none());
    assert!(v["global"].is_object());
    assert_eq!(v["global"]["total_requests"], json!(3));
    assert_eq!(v["global"]["failed_requests"], json!(1));
    assert!(v["global"].get("status").is_none());

    let endpoints = v["endpoints"].as_array().unwrap();
    assert_eq!(endpoints.len(), 2);
    let root = endpoints.iter().find(|e| e["name"] == "Get root").unwrap();
    assert_eq!(root["total_requests"], json!(2));
    assert_eq!(root["successful_requests"], json!(2));
    assert!(root["rps"].is_f64());
    assert!(root["failures_per_sec"].is_f64());
  }

  #[test]
  fn idle_interval_is_zeroed() {
    let line = build_interval_record(&[], &[], 2, 1.0, 2.0);
    let v: Value = serde_json::from_str(&line).unwrap();

    assert_eq!(v["version"], json!(SCHEMA_VERSION));
    assert_eq!(v["interval"], json!(2));
    assert_eq!(v["endpoints"].as_array().unwrap().len(), 0);
    assert_eq!(v["global"]["total_requests"], json!(0));
    assert_eq!(v["global"]["rps"], json!(0.0));
  }

  #[test]
  fn final_record_carries_status_and_terminates() {
    let reports = vec![report("Get root", 3.2, 200, 0.1)];
    let line = build_final_record(&reports, &[], 1.0, FinalStatus::Completed);
    let v: Value = serde_json::from_str(&line).unwrap();

    assert_eq!(v["version"], json!(SCHEMA_VERSION));
    assert_eq!(v["final"], json!(true));
    assert_eq!(v["global"]["status"], json!("completed"));
    assert_eq!(v["global"]["total_requests"], json!(1));
    assert!(v["global"]["failures_per_sec"].is_f64());
    assert!(v["global"]["duration_sec"].is_f64());
  }

  #[test]
  fn empty_run_final_record_is_zeroed_failed() {
    let line = build_final_record(&[], &[], 0.0, FinalStatus::Failed);
    let v: Value = serde_json::from_str(&line).unwrap();

    assert_eq!(v["final"], json!(true));
    assert_eq!(v["endpoints"].as_array().unwrap().len(), 0);
    assert_eq!(v["global"]["total_requests"], json!(0));
    assert_eq!(v["global"]["status"], json!("failed"));
  }

  #[test]
  fn success_codes_drive_failed_requests() {
    // Default success policy is 2xx; 3xx/4xx/5xx count as failures.
    let slice = vec![report("A", 1.0, 204, 0.1), report("B", 2.0, 302, 0.2), report("C", 3.0, 500, 0.3)];
    let line = build_final_record(&slice, &[], 1.0, FinalStatus::Completed);
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["global"]["failed_requests"], json!(2));
  }

  #[tokio::test]
  async fn finalize_emits_zeroed_record_for_empty_store() {
    let store: Arc<Mutex<Vec<Report>>> = Arc::new(Mutex::new(Vec::new()));
    let stream = StatsStream::start(1, store.clone(), Arc::from(Vec::<u16>::new()));
    // finalize awaits the ticker; the ticker has nothing to emit.
    let result = stream.finalize(FinalStatus::Cancelled).await;
    assert!(result.is_ok());
  }
}
