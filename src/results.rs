use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use hdrhistogram::Histogram;

use crate::actions::Report;
use crate::config::ResultsConfig;

const NS_PER_MS: f64 = 1_000_000.0;

#[derive(Clone)]
pub struct Stats {
  pub name: String,
  pub total: usize,
  pub failure: usize,
  pub min_ms: f64,
  pub max_ms: f64,
  pub mean_ms: f64,
  pub median_ms: f64,
  pub stdev_ms: f64,
  pub p50_ms: f64,
  pub p66_ms: f64,
  pub p75_ms: f64,
  pub p80_ms: f64,
  pub p90_ms: f64,
  pub p95_ms: f64,
  pub p98_ms: f64,
  pub p99_ms: f64,
  pub p999_ms: f64,
  pub p9999_ms: f64,
  pub p100_ms: f64,
  pub rps: f64,
  pub failures_per_sec: f64,
  pub min_ttfb_ms: f64,
  pub max_ttfb_ms: f64,
  pub mean_ttfb_ms: f64,
  pub median_ttfb_ms: f64,
  pub total_upload: u64,
  pub total_download: u64,
  pub total_size: u64,
  pub avg_upload: f64,
  pub avg_download: f64,
  pub avg_size: f64,
}

pub fn generate(reports: &[Vec<Report>], duration: f64, config: &ResultsConfig) {
  if duration == 0.0 {
    return;
  }

  fs::create_dir_all(&config.output_dir).expect("Unable to create results directory");

  let all_reports: Vec<Report> = reports.iter().flat_map(|v| v.iter().cloned()).collect();
  let stats = compute_all_stats(&all_reports, duration);
  let report_refs: Vec<&Report> = all_reports.iter().collect();

  if config.csv {
    write_csv(&stats, &config.output_dir);
  }

  if config.html {
    write_html(&stats, &report_refs, duration, &config.output_dir);
  }
}

fn compute_all_stats(reports: &[Report], duration: f64) -> Vec<Stats> {
  let mut by_name: HashMap<String, Vec<&Report>> = HashMap::new();

  for report in reports {
    by_name.entry(report.name.clone()).or_default().push(report);
  }

  let mut stats: Vec<Stats> = by_name
    .iter()
    .map(|(name, reps)| compute_stats(name, reps, duration))
    .collect();

  stats.sort_by_key(|s| s.name.clone());
  let total_refs: Vec<&Report> = reports.iter().collect();
  stats.push(compute_stats("Total", &total_refs, duration));
  stats
}

fn compute_stats(name: &str, reports: &[&Report], duration: f64) -> Stats {
  let mut hist = Histogram::<u64>::new_with_bounds(1, 60 * 60 * 1_000_000_000, 2).unwrap();
  let mut ttfb_hist = Histogram::<u64>::new_with_bounds(1, 60 * 60 * 1_000_000_000, 2).unwrap();
  let mut failure = 0usize;
  let mut min_ms = f64::INFINITY;
  let mut max_ms = 0f64;
  let mut min_ttfb_ms = f64::INFINITY;
  let mut max_ttfb_ms = 0f64;
  let mut total_upload = 0u64;
  let mut total_download = 0u64;
  let mut total_size = 0u64;

  for report in reports {
    hist += (report.duration * NS_PER_MS) as u64;
    ttfb_hist += (report.metrics.time_starttransfer_ms * NS_PER_MS) as u64;

    if report.status < 200 || report.status >= 300 {
      failure += 1;
    }

    if report.duration < min_ms {
      min_ms = report.duration;
    }

    if report.duration > max_ms {
      max_ms = report.duration;
    }

    if report.metrics.time_starttransfer_ms < min_ttfb_ms {
      min_ttfb_ms = report.metrics.time_starttransfer_ms;
    }

    if report.metrics.time_starttransfer_ms > max_ttfb_ms {
      max_ttfb_ms = report.metrics.time_starttransfer_ms;
    }

    total_upload += report.metrics.size_upload;
    total_download += report.metrics.size_download;
    total_size += report.metrics.size_total;
  }

  let total = reports.len();

  if min_ms == f64::INFINITY {
    min_ms = 0.0;
  }

  if min_ttfb_ms == f64::INFINITY {
    min_ttfb_ms = 0.0;
  }

  let total_f64 = total as f64;

  Stats {
    name: name.to_string(),
    total,
    failure,
    min_ms,
    max_ms,
    mean_ms: hist.mean() / NS_PER_MS,
    median_ms: hist.value_at_quantile(0.5) as f64 / NS_PER_MS,
    stdev_ms: hist.stdev() / NS_PER_MS,
    p50_ms: hist.value_at_quantile(0.50) as f64 / NS_PER_MS,
    p66_ms: hist.value_at_quantile(0.66) as f64 / NS_PER_MS,
    p75_ms: hist.value_at_quantile(0.75) as f64 / NS_PER_MS,
    p80_ms: hist.value_at_quantile(0.80) as f64 / NS_PER_MS,
    p90_ms: hist.value_at_quantile(0.90) as f64 / NS_PER_MS,
    p95_ms: hist.value_at_quantile(0.95) as f64 / NS_PER_MS,
    p98_ms: hist.value_at_quantile(0.98) as f64 / NS_PER_MS,
    p99_ms: hist.value_at_quantile(0.99) as f64 / NS_PER_MS,
    p999_ms: hist.value_at_quantile(0.999) as f64 / NS_PER_MS,
    p9999_ms: hist.value_at_quantile(0.9999) as f64 / NS_PER_MS,
    p100_ms: hist.value_at_quantile(1.0) as f64 / NS_PER_MS,
    rps: total as f64 / duration,
    failures_per_sec: failure as f64 / duration,
    min_ttfb_ms,
    max_ttfb_ms,
    mean_ttfb_ms: ttfb_hist.mean() / NS_PER_MS,
    median_ttfb_ms: ttfb_hist.value_at_quantile(0.5) as f64 / NS_PER_MS,
    total_upload,
    total_download,
    total_size,
    avg_upload: total_upload as f64 / total_f64,
    avg_download: total_download as f64 / total_f64,
    avg_size: total_size as f64 / total_f64,
  }
}

fn write_csv(stats: &[Stats], output_dir: &str) {
  let path = Path::new(output_dir).join("stats.csv");
  let mut file = File::create(&path).expect("Unable to create stats.csv");

  writeln!(
    file,
    "Name,Request Count,Failure Count,Median Response Time (ms),Average Response Time (ms),Min Response Time (ms),Max Response Time (ms),Standard Deviation (ms),Requests/s,Failures/s,50%,66%,75%,80%,90%,95%,98%,99%,99.9%,99.99%,100%,Min TTFB (ms),Max TTFB (ms),Mean TTFB (ms),Median TTFB (ms),Total Upload (bytes),Total Download (bytes),Total Size (bytes),Avg Upload (bytes),Avg Download (bytes),Avg Size (bytes)"
  )
  .unwrap();

  for stat in stats {
    writeln!(
      file,
      "{},{},{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{},{},{},{:.2},{:.2},{:.2}",
      stat.name,
      stat.total,
      stat.failure,
      stat.median_ms,
      stat.mean_ms,
      stat.min_ms,
      stat.max_ms,
      stat.stdev_ms,
      stat.rps,
      stat.failures_per_sec,
      stat.p50_ms,
      stat.p66_ms,
      stat.p75_ms,
      stat.p80_ms,
      stat.p90_ms,
      stat.p95_ms,
      stat.p98_ms,
      stat.p99_ms,
      stat.p999_ms,
      stat.p9999_ms,
      stat.p100_ms,
      stat.min_ttfb_ms,
      stat.max_ttfb_ms,
      stat.mean_ttfb_ms,
      stat.median_ttfb_ms,
      stat.total_upload,
      stat.total_download,
      stat.total_size,
      stat.avg_upload,
      stat.avg_download,
      stat.avg_size
    )
    .unwrap();
  }
}

fn write_html(stats: &[Stats], reports: &[&Report], duration: f64, output_dir: &str) {
  let path = Path::new(output_dir).join("report.html");
  let mut file = File::create(&path).expect("Unable to create report.html");

  let stats_rows: String = stats
    .iter()
    .map(|s| {
      format!(
        "<tr><td>{name}</td><td>{total}</td><td>{failure}</td><td>{median:.2}</td><td>{mean:.2}</td><td>{min:.2}</td><td>{max:.2}</td><td>{rps:.2}</td><td>{failures:.2}</td><td>{min_ttfb:.2}</td><td>{max_ttfb:.2}</td><td>{mean_ttfb:.2}</td><td>{median_ttfb:.2}</td><td>{avg_upload:.2}</td><td>{avg_download:.2}</td><td>{avg_size:.2}</td></tr>",
        name = html_escape(&s.name),
        total = s.total,
        failure = s.failure,
        median = s.median_ms,
        mean = s.mean_ms,
        min = s.min_ms,
        max = s.max_ms,
        rps = s.rps,
        failures = s.failures_per_sec,
        min_ttfb = s.min_ttfb_ms,
        max_ttfb = s.max_ttfb_ms,
        mean_ttfb = s.mean_ttfb_ms,
        median_ttfb = s.median_ttfb_ms,
        avg_upload = s.avg_upload,
        avg_download = s.avg_download,
        avg_size = s.avg_size
      )
    })
    .collect();

  let failures = compute_failures(reports);
  let failure_rows: String = failures
    .iter()
    .map(|(name, count)| format!("<tr><td>{name}</td><td>{count}</td></tr>", name = html_escape(name), count = count))
    .collect();

  let rps_chart = rps_over_time_svg(reports, duration);
  let avg_chart = average_response_time_svg(stats);

  let html = format!(
    r##"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Drill Report</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 2rem; color: #333; }}
h1, h2 {{ color: #222; }}
table {{ border-collapse: collapse; margin: 1rem 0; width: 100%; }}
th, td {{ border: 1px solid #ddd; padding: 0.5rem; text-align: right; }}
th {{ background: #f5f5f5; text-align: left; }}
td:first-child, th:first-child {{ text-align: left; }}
.chart {{ margin: 1.5rem 0; }}
svg {{ display: block; }}
</style>
</head>
<body>
<h1>Drill Load Test Report</h1>
<p>Duration: <strong>{duration:.2}s</strong></p>

<h2>Request Statistics</h2>
<table>
<thead><tr><th>Name</th><th>Requests</th><th>Failures</th><th>Median (ms)</th><th>Average (ms)</th><th>Min (ms)</th><th>Max (ms)</th><th>Req/s</th><th>Fail/s</th><th>Min TTFB (ms)</th><th>Max TTFB (ms)</th><th>Mean TTFB (ms)</th><th>Median TTFB (ms)</th><th>Avg Upload (bytes)</th><th>Avg Download (bytes)</th><th>Avg Size (bytes)</th></tr></thead>
<tbody>{stats_rows}</tbody>
</table>

<h2>Requests Per Second Over Time</h2>
<div class="chart">{rps_chart}</div>

<h2>Average Response Time Per Request</h2>
<div class="chart">{avg_chart}</div>

<h2>Failures</h2>
<table>
<thead><tr><th>Name</th><th>Count</th></tr></thead>
<tbody>{failure_rows}</tbody>
</table>
</body>
</html>"##,
    duration = duration,
    stats_rows = stats_rows,
    rps_chart = rps_chart,
    avg_chart = avg_chart,
    failure_rows = failure_rows
  );

  file.write_all(html.as_bytes()).unwrap();
}

fn compute_failures(reports: &[&Report]) -> Vec<(String, usize)> {
  let mut counts: HashMap<String, usize> = HashMap::new();

  for report in reports {
    if report.status < 200 || report.status >= 300 {
      *counts.entry(report.name.clone()).or_insert(0) += 1;
    }
  }

  let mut failures: Vec<(String, usize)> = counts.into_iter().collect();
  failures.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
  failures
}

fn html_escape(s: &str) -> String {
  s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn rps_over_time_svg(reports: &[&Report], duration: f64) -> String {
  if reports.is_empty() {
    return "<p>No data</p>".to_string();
  }

  let start = reports.iter().map(|r| r.timestamp).fold(f64::INFINITY, f64::min);
  let bucket_size = 1.0f64;
  let bucket_count = (duration / bucket_size).ceil() as usize;
  let mut buckets = vec![0usize; bucket_count.max(1)];

  for report in reports {
    let offset = report.timestamp - start;
    let idx = (offset / bucket_size) as usize;
    if idx < buckets.len() {
      buckets[idx] += 1;
    }
  }

  let max_rps = *buckets.iter().max().unwrap_or(&1) as f64;
  let width = 800;
  let height = 300;
  let padding = 50;
  let chart_width = width - padding * 2;
  let chart_height = height - padding * 2;

    let points: Vec<String> = buckets
    .iter()
    .enumerate()
    .map(|(i, &count)| {
      let x = padding as f64 + (i as f64 / buckets.len() as f64) * chart_width as f64;
      let y = padding as f64 + chart_height as f64 - (count as f64 / max_rps) * chart_height as f64;
      format!("{:.1},{:.1}", x, y)
    })
    .collect();

  let polyline = points.join(" ");

  let mut lines = String::new();
  for i in 0..=5 {
    let y = padding + (chart_height as f64 * i as f64 / 5.0) as usize;
    let value = max_rps * (1.0 - i as f64 / 5.0);
    lines.push_str(&format!(
      r##"<line x1="{pad}" y1="{y}" x2="{right}" y2="{y}" stroke="#eee" stroke-width="1"/>
<text x="{pad2}" y="{y2}" font-size="10" fill="#666" text-anchor="end">{value:.0}</text>"##,
      pad = padding,
      y = y,
      right = width - padding,
      pad2 = padding - 5,
      y2 = y + 3,
      value = value
    ));
  }

  format!(
    r##"<svg width="{width}" height="{height}" xmlns="http://www.w3.org/2000/svg">
<rect width="100%" height="100%" fill="#fafafa"/>
{lines}
<polyline points="{polyline}" fill="none" stroke="#4a90d9" stroke-width="2"/>
<text x="{mid}" y="{bottom}" font-size="12" fill="#666" text-anchor="middle">Time (seconds)</text>
</svg>"##,
    width = width,
    height = height,
    lines = lines,
    polyline = polyline,
    mid = width / 2,
    bottom = height - 10
  )
}

fn average_response_time_svg(stats: &[Stats]) -> String {
  let per_request: Vec<&Stats> = stats.iter().filter(|s| s.name != "Total").collect();

  if per_request.is_empty() {
    return "<p>No data</p>".to_string();
  }

  let max_avg = per_request.iter().map(|s| s.mean_ms).fold(0.0, f64::max);
  let width = 800;
  let height = 300;
  let padding = 60;
  let chart_width = width - padding * 2;
  let chart_height = height - padding * 2;
  let bar_width = chart_width as f64 / per_request.len() as f64 * 0.6;
  let step = chart_width as f64 / per_request.len() as f64;

  let mut bars = String::new();
  for (i, stat) in per_request.iter().enumerate() {
    let bar_height = if max_avg > 0.0 {
      (stat.mean_ms / max_avg) * chart_height as f64
    } else {
      0.0
    };

    let x = padding as f64 + (i as f64 * step) + (step - bar_width) / 2.0;
    let y = padding as f64 + chart_height as f64 - bar_height;

    bars.push_str(&format!(
      r##"<rect x="{x:.1}" y="{y:.1}" width="{bar_width:.1}" height="{bar_height:.1}" fill="#4a90d9"/>
<text x="{x2:.1}" y="{y2}" font-size="10" fill="#666" text-anchor="middle" transform="rotate(-30 {x2:.1},{y2})">{label}</text>
<text x="{x3:.1}" y="{y3:.1}" font-size="10" fill="#fff" text-anchor="middle">{value:.1}</text>"##,
      x = x,
      y = y,
      bar_width = bar_width,
      bar_height = bar_height,
      x2 = x + bar_width / 2.0,
      y2 = padding + chart_height + 20,
      label = html_escape(&stat.name),
      x3 = x + bar_width / 2.0,
      y3 = y + 15.0,
      value = stat.mean_ms
    ));
  }

  let mut lines = String::new();
  for i in 0..=5 {
    let y = padding + (chart_height as f64 * i as f64 / 5.0) as usize;
    let value = max_avg * (1.0 - i as f64 / 5.0);
    lines.push_str(&format!(
      r##"<line x1="{pad}" y1="{y}" x2="{right}" y2="{y}" stroke="#eee" stroke-width="1"/>
<text x="{pad2}" y="{y2}" font-size="10" fill="#666" text-anchor="end">{value:.0}</text>"##,
      pad = padding,
      y = y,
      right = width - padding,
      pad2 = padding - 5,
      y2 = y + 3,
      value = value
    ));
  }

  format!(
    r##"<svg width="{width}" height="{height}" xmlns="http://www.w3.org/2000/svg">
<rect width="100%" height="100%" fill="#fafafa"/>
{lines}
{bars}
<text x="{mid}" y="{bottom}" font-size="12" fill="#666" text-anchor="middle">Average Response Time (ms)</text>
</svg>"##,
    width = width,
    height = height,
    lines = lines,
    bars = bars,
    mid = width / 2,
    bottom = height - 10
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  fn report(name: &str, duration: f64, status: u16, timestamp: f64) -> Report {
    Report {
      name: name.to_string(),
      duration,
      status,
      timestamp,
      metrics: Default::default(),
    }
  }

  #[test]
  fn computes_basic_stats() {
    let reports: Vec<Report> = vec![
      report("a", 10.0, 200, 0.0),
      report("a", 20.0, 200, 1.0),
      report("a", 30.0, 500, 2.0),
    ];
    let report_refs: Vec<&Report> = reports.iter().collect();

    let stats = compute_stats("a", &report_refs, 3.0);

    assert_eq!(stats.total, 3);
    assert_eq!(stats.failure, 1);
    assert_eq!(stats.min_ms, 10.0);
    assert_eq!(stats.max_ms, 30.0);
    assert_eq!(stats.rps, 1.0);
  }

  #[test]
  fn generates_csv_file() {
    let reports: Vec<Vec<Report>> = vec![
      vec![report("a", 10.0, 200, 0.0)],
      vec![report("a", 20.0, 200, 1.0)],
      vec![report("b", 30.0, 500, 2.0)],
    ];

    let tmp = tempfile::tempdir().unwrap();
    let config = ResultsConfig {
      output_dir: tmp.path().to_str().unwrap().to_string(),
      csv: true,
      html: false,
    };

    generate(&reports, 3.0, &config);

    let csv_path = tmp.path().join("stats.csv");
    assert!(csv_path.exists());

    let content = fs::read_to_string(csv_path).unwrap();
    assert!(content.contains("Name,Request Count"));
    assert!(content.contains("Total"));
  }

  #[test]
  fn generates_html_file() {
    let reports: Vec<Vec<Report>> = vec![
      vec![report("a", 10.0, 200, 0.0)],
      vec![report("a", 20.0, 200, 1.0)],
      vec![report("b", 30.0, 500, 2.0)],
    ];

    let tmp = tempfile::tempdir().unwrap();
    let config = ResultsConfig {
      output_dir: tmp.path().to_str().unwrap().to_string(),
      csv: false,
      html: true,
    };

    generate(&reports, 3.0, &config);

    let html_path = tmp.path().join("report.html");
    assert!(html_path.exists());

    let content = fs::read_to_string(html_path).unwrap();
    assert!(content.contains("Drill Load Test Report"));
    assert!(content.contains("Request Statistics"));
  }
}
