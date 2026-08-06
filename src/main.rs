mod actions;
mod benchmark;
mod checker;
mod config;
mod expandable;
mod faker;
mod interpolator;
mod metrics;
mod reader;
mod results;
mod tags;
mod writer;

use crate::actions::Report;
use clap::{Arg, ArgAction, Command, crate_version};
use colored::*;
use hdrhistogram::Histogram;
use linked_hash_map::LinkedHashMap;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process;

fn main() {
  let matches = app_args();
  let benchmark_file = matches.get_one::<String>("benchmark").unwrap().as_str();
  let report_path_option = matches.get_one::<String>("report").map(|s| s.as_str());
  let stats_option = matches.get_flag("stats");
  let stats_json = matches.get_flag("stats-json");
  let stats_interval = matches.get_one::<u64>("stats-interval").copied();
  let stats_csv = matches.get_flag("stats-csv");
  let compare_path_option = matches.get_one::<String>("compare").map(|s| s.as_str());
  let threshold_option = matches.get_one::<String>("threshold").map(|s| s.as_str());
  let no_check_certificate = matches.get_flag("no-check-certificate");
  let relaxed_interpolations = matches.get_flag("relaxed-interpolations");
  let quiet = matches.get_flag("quiet");
  let nanosec = matches.get_flag("nanosec");
  let timeout = matches.get_one::<String>("timeout").map(|s| s.as_str());
  let verbose = matches.get_flag("verbose");
  let tags_option = matches.get_one::<String>("tags").map(|s| s.as_str());
  let skip_tags_option = matches.get_one::<String>("skip-tags").map(|s| s.as_str());
  let list_tags = matches.get_flag("list-tags");
  let list_tasks = matches.get_flag("list-tasks");
  let vars_option = matches.get_one::<String>("vars").map(|s| s.as_str());
  let threads = matches.get_one::<usize>("threads").copied();
  let conn_per_iter = if matches.contains_id("new-conn-per-iter") { Some(true) } else { None };
  let continue_on_assert_fail = matches.get_flag("continue-on-assert-fail");
  let run_time = matches.get_one::<u64>("run-time").copied();

  #[cfg(windows)]
  let _ = control::set_virtual_terminal(true);

  if list_tags {
    tags::list_benchmark_file_tags(benchmark_file);
    process::exit(0);
  };

  let tags = tags::Tags::new(tags_option, skip_tags_option);

  if list_tasks {
    tags::list_benchmark_file_tasks(benchmark_file, &tags);
    process::exit(0);
  };

  let benchmark_result = benchmark::execute(benchmark_file, vars_option, report_path_option, relaxed_interpolations, no_check_certificate, quiet, nanosec, timeout, verbose, &tags, threads, conn_per_iter, continue_on_assert_fail, run_time);
  let list_reports = benchmark_result.reports;
  let duration = benchmark_result.duration;

  show_stats(&list_reports, stats_option, stats_json, stats_csv, nanosec, duration, stats_interval);
  compare_benchmark(&list_reports, compare_path_option, threshold_option);

  if benchmark_result.assertion_failures > 0 {
    eprintln!("{} {} assertion(s) failed", "Assertion results:".red().bold(), benchmark_result.assertion_failures.to_string().purple());
    process::exit(1);
  }

  process::exit(0)
}

fn app_args() -> clap::ArgMatches {
  Command::new("drill")
    .version(crate_version!())
    .about("HTTP load testing application written in Rust inspired by Ansible syntax")
    .arg(Arg::new("benchmark").help("Sets the benchmark file").long("benchmark").short('b').required(true))
    .arg(Arg::new("stats").short('s').long("stats").help("Shows request statistics").action(ArgAction::SetTrue).conflicts_with("compare"))
    .arg(Arg::new("stats-json").long("stats-json").help("Outputs statistics as JSON Lines (NDJSON) to stdout").action(ArgAction::SetTrue))
    .arg(Arg::new("stats-interval").long("stats-interval").value_parser(clap::value_parser!(u64)).help("Interval in seconds for streaming statistics (default: 1, requires --stats-json)"))
    .arg(Arg::new("stats-csv").long("stats-csv").help("Outputs statistics as CSV to stdout").action(ArgAction::SetTrue))
    .arg(Arg::new("report").short('r').long("report").help("Sets a report file").conflicts_with("compare"))
    .arg(Arg::new("compare").short('c').long("compare").help("Sets a compare file").conflicts_with("report"))
    .arg(Arg::new("threshold").short('t').long("threshold").help("Sets a threshold value in ms amongst the compared file").conflicts_with("report"))
    .arg(Arg::new("relaxed-interpolations").long("relaxed-interpolations").help("Do not panic if an interpolation is not present. (Not recommended)").action(ArgAction::SetTrue))
    .arg(Arg::new("no-check-certificate").long("no-check-certificate").help("Disables SSL certification check. (Not recommended)").action(ArgAction::SetTrue))
    .arg(Arg::new("tags").long("tags").help("Tags to include"))
    .arg(Arg::new("skip-tags").long("skip-tags").help("Tags to exclude"))
    .arg(Arg::new("list-tags").long("list-tags").help("List all benchmark tags").action(ArgAction::SetTrue).conflicts_with_all(["tags", "skip-tags"]))
    .arg(Arg::new("list-tasks").long("list-tasks").help("List benchmark tasks (executes --tags/--skip-tags filter)").action(ArgAction::SetTrue))
    .arg(Arg::new("quiet").short('q').long("quiet").help("Disables output").action(ArgAction::SetTrue))
    .arg(Arg::new("timeout").short('o').long("timeout").help("Set timeout in seconds for all requests"))
    .arg(Arg::new("vars").long("vars").help("Sets a YAML file with variables to inject into interpolations"))
    .arg(Arg::new("nanosec").short('n').long("nanosec").help("Shows statistics in nanoseconds").action(ArgAction::SetTrue))
    .arg(Arg::new("verbose").short('v').long("verbose").help("Toggle verbose output").action(ArgAction::SetTrue))
    .arg(Arg::new("threads").long("threads").value_parser(clap::value_parser!(usize)).help("Number of worker threads for the tokio runtime (defaults to CPU core count, capped at the number of CPU cores)"))
    .arg(Arg::new("new-conn-per-iter").long("new-conn-per-iter").action(ArgAction::SetTrue).help("Create a fresh HTTP connection (new reqwest client, fresh DNS lookup) for every iteration instead of reusing connections across iterations"))
    .arg(Arg::new("continue-on-assert-fail").long("continue-on-assert-fail").action(ArgAction::SetTrue).help("Record assertion failures and continue the benchmark instead of aborting on the first failure"))
    .arg(Arg::new("run-time").long("run-time").value_parser(clap::value_parser!(u64)).help("Wall-clock duration limit in seconds after which the benchmark stops accepting new iterations"))
    .get_matches()
}

struct DrillStats {
  total_requests: usize,
  successful_requests: usize,
  failed_requests: usize,
  hist: Histogram<u64>,
}

impl DrillStats {
  fn mean_duration(&self) -> f64 {
    self.hist.mean() / 1_000.0
  }
  fn median_duration(&self) -> f64 {
    self.hist.value_at_quantile(0.5) as f64 / 1_000.0
  }
  fn stdev_duration(&self) -> f64 {
    self.hist.stdev() / 1_000.0
  }
  fn value_at_quantile(&self, quantile: f64) -> f64 {
    self.hist.value_at_quantile(quantile) as f64 / 1_000.0
  }
  fn max_duration(&self) -> f64 {
    self.hist.max() as f64 / 1_000.0
  }
}

fn compute_stats(sub_reports: &[Report]) -> DrillStats {
  let mut hist = Histogram::<u64>::new_with_bounds(1, 60 * 60 * 1_000_000, 2).unwrap();
  let mut group_by_status = HashMap::new();

  for req in sub_reports {
    group_by_status.entry(req.status / 100).or_insert_with(Vec::new).push(req);
  }

  for r in sub_reports.iter() {
    hist += (r.duration * 1_000.0) as u64;
  }

  let total_requests = sub_reports.len();
  let successful_requests = group_by_status.entry(2).or_insert_with(Vec::new).len();
  let failed_requests = total_requests - successful_requests;

  DrillStats {
    total_requests,
    successful_requests,
    failed_requests,
    hist,
  }
}

fn format_time(tdiff: f64, nanosec: bool) -> String {
  if nanosec {
    (1_000_000.0 * tdiff).round().to_string() + "ns"
  } else {
    tdiff.round().to_string() + "ms"
  }
}

fn show_stats(list_reports: &[Vec<Report>], stats_option: bool, stats_json: bool, stats_csv: bool, nanosec: bool, duration: f64, stats_interval: Option<u64>) {
  if !stats_option && !stats_json && !stats_csv {
    return;
  }

  let mut group_by_name = LinkedHashMap::new();

  for req in list_reports.concat() {
    group_by_name.entry(req.name.clone()).or_insert_with(Vec::new).push(req);
  }

  // compute stats per name
  for (name, reports) in group_by_name {
    let substats = compute_stats(&reports);
    println!();
    println!("{:width$} {:width2$} {}", name.green(), "Total requests".yellow(), substats.total_requests.to_string().purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Successful requests".yellow(), substats.successful_requests.to_string().purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Failed requests".yellow(), substats.failed_requests.to_string().purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Median time per request".yellow(), format_time(substats.median_duration(), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Average time per request".yellow(), format_time(substats.mean_duration(), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Sample standard deviation".yellow(), format_time(substats.stdev_duration(), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "99.0'th percentile".yellow(), format_time(substats.value_at_quantile(0.99), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "99.5'th percentile".yellow(), format_time(substats.value_at_quantile(0.995), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "99.9'th percentile".yellow(), format_time(substats.value_at_quantile(0.999), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "95.0'th percentile".yellow(), format_time(substats.value_at_quantile(0.95), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Max time per request".yellow(), format_time(substats.max_duration(), nanosec).purple(), width = 25, width2 = 25);
    let endpoint_rps = substats.total_requests as f64 / duration;
    println!("{:width$} {:width2$} {} {}", name.green(), "Requests per second".yellow(), format!("{endpoint_rps:.2}").purple(), "[#/sec]".purple(), width = 25, width2 = 25);
  }

  // compute global stats
  let allreports = list_reports.concat();
  let global_stats = compute_stats(&allreports);
  let requests_per_second = global_stats.total_requests as f64 / duration;

  println!();
  println!("{:width2$} {} {}", "Time taken for tests".yellow(), format!("{duration:.1}").purple(), "seconds".purple(), width2 = 25);
  println!("{:width2$} {}", "Total requests".yellow(), global_stats.total_requests.to_string().purple(), width2 = 25);
  println!("{:width2$} {}", "Successful requests".yellow(), global_stats.successful_requests.to_string().purple(), width2 = 25);
  println!("{:width2$} {}", "Failed requests".yellow(), global_stats.failed_requests.to_string().purple(), width2 = 25);
  println!("{:width2$} {} {}", "Requests per second".yellow(), format!("{requests_per_second:.2}").purple(), "[#/sec]".purple(), width2 = 25);
  println!("{:width2$} {}", "Median time per request".yellow(), format_time(global_stats.median_duration(), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "Average time per request".yellow(), format_time(global_stats.mean_duration(), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "Sample standard deviation".yellow(), format_time(global_stats.stdev_duration(), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "99.0'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.99), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "99.5'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.995), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "99.9'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.999), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "95.0'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.95), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "Max time per request".yellow(), format_time(global_stats.max_duration(), nanosec).purple(), width2 = 25);
  
  if stats_json {
    export_json(&list_reports, duration, nanosec, stats_interval.unwrap_or(1));
  }
  if stats_csv {
    export_csv(&list_reports, duration, nanosec);
  }
}

fn endpoint_stats_json(endpoints: &[(&String, &DrillStats)]) -> Value {
  let mut items = Vec::new();
  for (name, s) in endpoints {
    items.push(json!({
      "name": name,
      "total_requests": s.total_requests,
      "successful_requests": s.successful_requests,
      "failed_requests": s.failed_requests,
      "avg_ms": s.mean_duration(),
      "median_ms": s.median_duration(),
      "stdev_ms": s.stdev_duration(),
      "p50_ms": s.value_at_quantile(0.5),
      "p66_ms": s.value_at_quantile(0.66),
      "p75_ms": s.value_at_quantile(0.75),
      "p80_ms": s.value_at_quantile(0.80),
      "p90_ms": s.value_at_quantile(0.90),
      "p95_ms": s.value_at_quantile(0.95),
      "p98_ms": s.value_at_quantile(0.98),
      "p99_ms": s.value_at_quantile(0.99),
      "p999_ms": s.value_at_quantile(0.999),
      "p9999_ms": s.value_at_quantile(0.9999),
      "max_ms": s.max_duration(),
    }));
  }
  Value::Array(items)
}

fn export_json(list_reports: &[Vec<Report>], duration: f64, _nanosec: bool, interval_secs: u64) {
  let all_reports: Vec<Report> = list_reports.iter().flat_map(|v| v.iter().cloned()).collect();
  if all_reports.is_empty() {
    return;
  }

  let time_start = all_reports.iter().map(|r| r.timestamp).fold(f64::INFINITY, f64::min);
  let total_intervals = ((duration / interval_secs as f64).ceil() as u64).max(1);
  let interval_duration = duration / total_intervals as f64;

  let mut interval_reports: Vec<Vec<Report>> = vec![Vec::new(); total_intervals as usize];
  for report in &all_reports {
    let elapsed = report.timestamp - time_start;
    let idx = ((elapsed / interval_duration) as usize).min(interval_reports.len() - 1);
    interval_reports[idx].push(report.clone());
  }

  for (i, slice) in interval_reports.iter().enumerate() {
    let mut by_name: LinkedHashMap<String, Vec<Report>> = LinkedHashMap::new();
    for report in slice {
      by_name.entry(report.name.clone()).or_default().push(report.clone());
    }
    let endpoint_stats: Vec<(String, DrillStats)> = by_name.iter().map(|(name, reps)| (name.clone(), compute_stats(reps))).collect();
    let ep_refs: Vec<(&String, &DrillStats)> = endpoint_stats.iter().map(|(n, s)| (n, s)).collect();
    let interval_line = json!({
      "interval": i + 1,
      "endpoints": endpoint_stats_json(&ep_refs),
      "time_elapsed_sec": (i as f64) * interval_duration + interval_duration,
    });
    println!("{}", serde_json::to_string(&interval_line).unwrap());
  }

  let mut by_name: LinkedHashMap<String, Vec<Report>> = LinkedHashMap::new();
  for report in &all_reports {
    by_name.entry(report.name.clone()).or_default().push(report.clone());
  }
  let endpoint_stats: Vec<(String, DrillStats)> = by_name.iter().map(|(name, reps)| (name.clone(), compute_stats(reps))).collect();
  let ep_refs: Vec<(&String, &DrillStats)> = endpoint_stats.iter().map(|(n, s)| (n, s)).collect();

  let global_drill = compute_stats(&all_reports);
  let global_rps = all_reports.len() as f64 / duration;

  let final_line = json!({
    "final": true,
    "endpoints": endpoint_stats_json(&ep_refs),
    "global": {
      "total_requests": global_drill.total_requests,
      "successful_requests": global_drill.successful_requests,
      "failed_requests": global_drill.failed_requests,
      "avg_ms": global_drill.mean_duration(),
      "median_ms": global_drill.median_duration(),
      "stdev_ms": global_drill.stdev_duration(),
      "p50_ms": global_drill.value_at_quantile(0.5),
      "p66_ms": global_drill.value_at_quantile(0.66),
      "p75_ms": global_drill.value_at_quantile(0.75),
      "p80_ms": global_drill.value_at_quantile(0.80),
      "p90_ms": global_drill.value_at_quantile(0.90),
      "p95_ms": global_drill.value_at_quantile(0.95),
      "p98_ms": global_drill.value_at_quantile(0.98),
      "p99_ms": global_drill.value_at_quantile(0.99),
      "p999_ms": global_drill.value_at_quantile(0.999),
      "p9999_ms": global_drill.value_at_quantile(0.9999),
      "max_ms": global_drill.max_duration(),
      "rps": global_rps,
      "duration_sec": duration,
      "time_elapsed_sec": duration,
    }
  });
  println!("{}", serde_json::to_string(&final_line).unwrap());
}

fn export_csv(list_reports: &[Vec<Report>], duration: f64, _nanosec: bool) {
  let all_reports: Vec<&Report> = list_reports.iter().flat_map(|v| v.iter()).collect();
  let mut by_name: LinkedHashMap<String, Vec<&Report>> = LinkedHashMap::new();
  
  for report in &all_reports {
    by_name.entry(report.name.clone()).or_default().push(*report);
  }
  
  println!("name,total_requests,successful_requests,failed_requests,avg_ms,median_ms,stdev_ms,p50_ms,p66_ms,p75_ms,p80_ms,p90_ms,p95_ms,p98_ms,p99_ms,p999_ms,p9999_ms,max_ms,rps,failures_per_sec");
  
  for (name, reports) in &by_name {
    let substats = compute_stats_for_export(reports, duration);
    println!("{},{},{},{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2}",
      name,
      substats.total_requests,
      substats.successful_requests,
      substats.failed_requests,
      substats.mean_duration(),
      substats.median_duration(),
      substats.stdev_duration(),
      substats.value_at_quantile(0.5),
      substats.value_at_quantile(0.66),
      substats.value_at_quantile(0.75),
      substats.value_at_quantile(0.80),
      substats.value_at_quantile(0.90),
      substats.value_at_quantile(0.95),
      substats.value_at_quantile(0.98),
      substats.value_at_quantile(0.99),
      substats.value_at_quantile(0.999),
      substats.value_at_quantile(0.9999),
      substats.max_duration(),
      substats.rps,
      substats.failures_per_sec
    );
  }
  
  let global_stats = compute_stats_for_export(&all_reports.iter().cloned().collect::<Vec<_>>(), duration);
  println!("Total,{},{},{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2}",
    global_stats.total_requests,
    global_stats.successful_requests,
    global_stats.failed_requests,
    global_stats.mean_duration(),
    global_stats.median_duration(),
    global_stats.stdev_duration(),
    global_stats.value_at_quantile(0.5),
    global_stats.value_at_quantile(0.66),
    global_stats.value_at_quantile(0.75),
    global_stats.value_at_quantile(0.80),
    global_stats.value_at_quantile(0.90),
    global_stats.value_at_quantile(0.95),
    global_stats.value_at_quantile(0.98),
    global_stats.value_at_quantile(0.99),
    global_stats.value_at_quantile(0.999),
    global_stats.value_at_quantile(0.9999),
    global_stats.max_duration(),
    global_stats.rps,
    global_stats.failures_per_sec
  );
}

struct ExportStats {
  total_requests: usize,
  successful_requests: usize,
  failed_requests: usize,
  hist: Histogram<u64>,
  rps: f64,
  failures_per_sec: f64,
}

impl ExportStats {
  fn mean_duration(&self) -> f64 {
    self.hist.mean() / 1_000.0
  }
  fn median_duration(&self) -> f64 {
    self.hist.value_at_quantile(0.5) as f64 / 1_000.0
  }
  fn stdev_duration(&self) -> f64 {
    self.hist.stdev() / 1_000.0
  }
  fn value_at_quantile(&self, quantile: f64) -> f64 {
    self.hist.value_at_quantile(quantile) as f64 / 1_000.0
  }
  fn max_duration(&self) -> f64 {
    self.hist.max() as f64 / 1_000.0
  }
}

fn compute_stats_for_export(reports: &[&Report], duration: f64) -> ExportStats {
  let mut hist = Histogram::<u64>::new_with_bounds(1, 60 * 60 * 1_000_000, 2).unwrap();
  let mut group_by_status = HashMap::new();
  
  for req in reports {
    group_by_status.entry(req.status / 100).or_insert_with(Vec::new).push(req);
  }
  
  for r in reports.iter() {
    hist += (r.duration * 1_000.0) as u64;
  }
  
  let total_requests = reports.len();
  let successful_requests = group_by_status.entry(2).or_insert_with(Vec::new).len();
  let failed_requests = total_requests - successful_requests;
  let rps = total_requests as f64 / duration;
  let failures_per_sec = failed_requests as f64 / duration;
  
  ExportStats {
    total_requests,
    successful_requests,
    failed_requests,
    hist,
    rps,
    failures_per_sec,
  }
}

fn compare_benchmark(list_reports: &[Vec<Report>], compare_path_option: Option<&str>, threshold_option: Option<&str>) {
  if let Some(compare_path) = compare_path_option {
    if let Some(threshold) = threshold_option {
      let compare_result = checker::compare(list_reports, compare_path, threshold);

      match compare_result {
        Ok(_) => process::exit(0),
        Err(_) => process::exit(1),
      }
    } else {
      panic!("Threshold needed!");
    }
  }
}
