mod actions;
mod benchmark;
mod checker;
mod config;
mod expandable;
mod interpolator;
mod reader;
mod tags;
mod writer;

use crate::actions::Report;
use clap::crate_version;
use clap::{App, Arg};
use colored::*;
use hdrhistogram::Histogram;
use linked_hash_map::LinkedHashMap;
use std::collections::HashMap;
use std::process;

fn main() {
  let matches = app_args();
  let benchmark_file = matches.value_of("benchmark").unwrap();
  let report_path_option = matches.value_of("report");
  let stats_option = matches.is_present("stats");
  let histogram_max_width = matches.value_of("histogram-max-width").unwrap_or("100").parse::<usize>().unwrap();
  let compare_path_option = matches.value_of("compare");
  let threshold_option = matches.value_of("threshold");
  let no_check_certificate = matches.is_present("no-check-certificate");
  let relaxed_interpolations = matches.is_present("relaxed-interpolations");
  let quiet = matches.is_present("quiet");
  let nanosec = matches.is_present("nanosec");
  let timeout = matches.value_of("timeout");
  let verbose = matches.is_present("verbose");
  let tags_option = matches.value_of("tags");
  let skip_tags_option = matches.value_of("skip-tags");
  let list_tags = matches.is_present("list-tags");
  let list_tasks = matches.is_present("list-tasks");

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

  let benchmark_result = benchmark::execute(benchmark_file, report_path_option, relaxed_interpolations, no_check_certificate, quiet, nanosec, timeout, verbose, &tags);
  let list_reports = benchmark_result.reports;
  let duration = benchmark_result.duration;

  show_stats(&list_reports, stats_option, nanosec, duration, histogram_max_width);
  compare_benchmark(&list_reports, compare_path_option, threshold_option);

  process::exit(0)
}

fn app_args<'a>() -> clap::ArgMatches<'a> {
  App::new("drill")
    .version(crate_version!())
    .about("HTTP load testing application written in Rust inspired by Ansible syntax")
    .arg(Arg::with_name("benchmark").help("Sets the benchmark file").long("benchmark").short("b").required(true).takes_value(true))
    .arg(Arg::with_name("stats").short("s").long("stats").help("Shows request statistics").takes_value(false).conflicts_with("compare"))
    .arg(Arg::with_name("report").short("r").long("report").help("Sets a report file").takes_value(true).conflicts_with("compare"))
    .arg(Arg::with_name("compare").short("c").long("compare").help("Sets a compare file").takes_value(true).conflicts_with("report"))
    .arg(Arg::with_name("threshold").short("t").long("threshold").help("Sets a threshold value in ms amongst the compared file").takes_value(true).conflicts_with("report"))
    .arg(Arg::with_name("relaxed-interpolations").long("relaxed-interpolations").help("Do not panic if an interpolation is not present. (Not recommended)").takes_value(false))
    .arg(Arg::with_name("no-check-certificate").long("no-check-certificate").help("Disables SSL certification check. (Not recommended)").takes_value(false))
    .arg(Arg::with_name("tags").long("tags").help("Tags to include").takes_value(true))
    .arg(Arg::with_name("skip-tags").long("skip-tags").help("Tags to exclude").takes_value(true))
    .arg(Arg::with_name("list-tags").long("list-tags").help("List all benchmark tags").takes_value(false).conflicts_with_all(&["tags", "skip-tags"]))
    .arg(Arg::with_name("list-tasks").long("list-tasks").help("List benchmark tasks (executes --tags/--skip-tags filter)").takes_value(false))
    .arg(Arg::with_name("quiet").short("q").long("quiet").help("Disables output").takes_value(false))
    .arg(Arg::with_name("timeout").short("o").long("timeout").help("Set timeout in seconds for all requests").takes_value(true))
    .arg(Arg::with_name("histogram-max-width").short("w").long("histogram-max-width").help("Set the maximum width of the histogram").takes_value(true))
    .arg(Arg::with_name("nanosec").short("n").long("nanosec").help("Shows statistics in nanoseconds").takes_value(false))
    .arg(Arg::with_name("verbose").short("v").long("verbose").help("Toggle verbose output").takes_value(false))
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
  fn max_duration(&self) -> f64 {
    self.hist.max() as f64 / 1_000.0
  }
  fn min_duration(&self) -> f64 {
    self.hist.min() as f64 / 1_000.0
  }
  fn stdev_duration(&self) -> f64 {
    self.hist.stdev() / 1_000.0
  }
  fn value_at_quantile(&self, quantile: f64) -> f64 {
    self.hist.value_at_quantile(quantile) as f64 / 1_000.0
  }
  fn print_histogram(&self, max_symbols: usize) {
    let max_value = self.hist.max();
    let min_value = self.hist.min();
    let bin_size = if max_value == min_value {
      1
    } else {
      (max_value - min_value) / 10
    };
    let max_range_string_length = format!("[{} - {}]", max_value / 1_000, (max_value + bin_size) / 1_000).len();

    // Collect counts for each bin
    let mut counts = vec![];
    for i in 0..10 {
      let lower_bound = min_value + i * bin_size;
      let upper_bound = std::cmp::min(lower_bound + bin_size, max_value + 1); // Ensure last bin includes max_value
      let count = self.hist.iter_recorded().filter(|v| v.value_iterated_to() >= lower_bound && v.value_iterated_to() < upper_bound).count();
      counts.push(count);
    }

    // Normalize counts
    let max_count = *counts.iter().max().unwrap_or(&1);
    let factor = if max_count > max_symbols {
      max_count as f64 / max_symbols as f64
    } else {
      1.0
    };

    for (i, &count) in counts.iter().enumerate() {
      let lower_bound = min_value + (i as u64) * bin_size;

      // If this is the last bin then the upper bound is max_value
      let upper_bound = if i == 9 {
        max_value + 1000
      } else {
        lower_bound + bin_size
      };

      let normalized_count = if factor > 1.0 {
        (count as f64 / factor).round() as usize
      } else {
        count
      };

      let range_string = format!("[{} - {}]", lower_bound / 1_000, upper_bound / 1_000);
      let range_string_padded = format!("{:width$}", range_string, width = max_range_string_length).yellow();
      println!("{}: {}", range_string_padded, "█".repeat(normalized_count).purple());
    }
  }
}

fn compute_stats(sub_reports: &[Report]) -> DrillStats {
  let mut hist = Histogram::<u64>::new_with_bounds(1, 60 * 60 * 1000, 2).unwrap();
  hist.auto(true); // Allow auto resizing or adding new values can result in unwrapping errors if
                   // they're out of bounds
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

fn show_stats(list_reports: &[Vec<Report>], stats_option: bool, nanosec: bool, duration: f64, histogram_max_width: usize) {
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
    println!("{:width$} {:width2$} {}", name.green(), "Min time per request".yellow(), format_time(substats.min_duration(), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Max time per request".yellow(), format_time(substats.max_duration(), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "50.0'th percentile".yellow(), format_time(substats.value_at_quantile(0.5), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "95.0'th percentile".yellow(), format_time(substats.value_at_quantile(0.95), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "99.0'th percentile".yellow(), format_time(substats.value_at_quantile(0.99), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "99.5'th percentile".yellow(), format_time(substats.value_at_quantile(0.995), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "99.9'th percentile".yellow(), format_time(substats.value_at_quantile(0.999), nanosec).purple(), width = 25, width2 = 25);
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
  println!("{:width2$} {}", "Min time per request".yellow(), format_time(global_stats.min_duration(), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "Max time per request".yellow(), format_time(global_stats.max_duration(), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "50.0'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.5), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "95.0'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.95), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "99.0'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.99), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "99.5'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.995), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "99.9'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.999), nanosec).purple(), width2 = 25);
  println!();
  println!("{}", "Request Histogram".yellow());
  println!();
  global_stats.print_histogram(histogram_max_width);
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
