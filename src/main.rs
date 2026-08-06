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
use std::collections::HashMap;
use std::process;

fn main() {
  let matches = app_args();
  let benchmark_file = matches.get_one::<String>("benchmark").unwrap().as_str();
  let report_path_option = matches.get_one::<String>("report").map(|s| s.as_str());
  let stats_option = matches.get_flag("stats");
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

  let benchmark_result = benchmark::execute(benchmark_file, vars_option, report_path_option, relaxed_interpolations, no_check_certificate, quiet, nanosec, timeout, verbose, &tags, threads, conn_per_iter);
  let list_reports = benchmark_result.reports;
  let duration = benchmark_result.duration;

  show_stats(&list_reports, stats_option, nanosec, duration);
  compare_benchmark(&list_reports, compare_path_option, threshold_option);

  process::exit(0)
}

fn app_args() -> clap::ArgMatches {
  Command::new("drill")
    .version(crate_version!())
    .about("HTTP load testing application written in Rust inspired by Ansible syntax")
    .arg(Arg::new("benchmark").help("Sets the benchmark file").long("benchmark").short('b').required(true))
    .arg(Arg::new("stats").short('s').long("stats").help("Shows request statistics").action(ArgAction::SetTrue).conflicts_with("compare"))
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

fn show_stats(list_reports: &[Vec<Report>], stats_option: bool, nanosec: bool, duration: f64) {
  if !stats_option {
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
    println!("{:width$} {:width2$} {}", name.green(), "Max time per request".yellow(), format_time(substats.max_duration(), nanosec).purple(), width = 25, width2 = 25);
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
  println!("{:width2$} {}", "Max time per request".yellow(), format_time(global_stats.max_duration(), nanosec).purple(), width2 = 25);
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
