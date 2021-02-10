mod actions;
mod benchmark;
mod checker;
mod config;
mod expandable;
mod interpolator;
mod reader;
mod writer;

use crate::actions::Report;
use crate::benchmark::BenchmarkOptions;
use clap::crate_version;
use clap::{App, Arg};
use colored::*;
use linked_hash_map::LinkedHashMap;
use std::collections::HashMap;
use std::f64;
use std::process;

fn main() {
  // validate user command line input
  let matches = app_args();
  let options = BenchmarkOptions {
    benchmark_path_option: matches.value_of("benchmark"),
    report_path_option: matches.value_of("report"),
    stats: matches.is_present("stats"),
    compare_path_option: matches.value_of("compare"),
    threshold_option: if let Some(threshold) = matches.value_of("threshold") {
      Some(threshold.parse::<f64>().expect("Command line parameter 'threshold' value must be a positive numerical value"))
    } else {
      None
    },
    no_check_certificate: matches.is_present("no-check-certificate"),
    relaxed_interpolations: matches.is_present("relaxed-interpolations"),
    quiet: matches.is_present("quiet"),
    nanosec: matches.is_present("nanosec"),
    concurrency_option: if let Some(concurrency) = matches.value_of("concurrency") {
      Some(concurrency.parse::<usize>().expect("Command line parameter 'concurrency' value must be a positive integer"))
    } else {
      None
    },
    base_url_option: matches.value_of("url"),
    iterations_option: if let Some(iterations) = matches.value_of("iterations") {
      Some(iterations.parse::<usize>().expect("Command line parameter 'iterations' value must be a positive integer"))
    } else {
      None
    },
    rampup_option: if let Some(rampup) = matches.value_of("rampup") {
      Some(rampup.parse::<usize>().expect("Command line parameter 'rampup' value must be a positive integer"))
    } else {
      None
    },
  };

  // run the benchmark
  let benchmark_result = benchmark::execute(&options);

  // process reports and statistics
  let list_reports = benchmark_result.reports;
  let duration = benchmark_result.duration;
  show_stats(&list_reports, options.stats, options.nanosec, duration);
  compare_benchmark(&list_reports, options.compare_path_option, options.threshold_option);

  process::exit(0)
}

fn app_args<'a>() -> clap::ArgMatches<'a> {
  App::new("drill")
    .version(crate_version!())
    .about("HTTP load testing application written in Rust inspired by Ansible syntax")
    .arg(Arg::with_name("benchmark").help("Sets the benchmark file").long("benchmark").short("b").required_unless("url").takes_value(true))
    .arg(Arg::with_name("stats").short("s").long("stats").help("Shows request statistics").takes_value(false).conflicts_with("compare"))
    .arg(Arg::with_name("report").short("r").long("report").help("Sets a report file").takes_value(true).conflicts_with("compare"))
    .arg(Arg::with_name("compare").short("c").long("compare").help("Sets a compare file").takes_value(true).conflicts_with("report"))
    .arg(Arg::with_name("threshold").short("t").long("threshold").help("Sets a threshold value in ms amongst the compared file").takes_value(true).conflicts_with("report"))
    .arg(Arg::with_name("relaxed-interpolations").long("relaxed-interpolations").help("Do not panic if an interpolation is not present. (Not recommended)").takes_value(false))
    .arg(Arg::with_name("no-check-certificate").long("no-check-certificate").help("Disables SSL certification check. (Not recommended)").takes_value(false))
    .arg(Arg::with_name("quiet").short("q").long("quiet").help("Disables output").takes_value(false))
    .arg(Arg::with_name("nanosec").short("n").long("nanosec").help("Shows statistics in nanoseconds").takes_value(false))
    .arg(Arg::with_name("concurrency").short("p").long("concurrency").help("Sets the number of parallel/concurrent requests (overrides benchmark file)").takes_value(true))
    .arg(Arg::with_name("iterations").short("i").long("iterations").help("Sets the total number of requests to perform (overrides benchmark file)").takes_value(true))
    .arg(Arg::with_name("url").short("u").long("url").help("Sets the base URL for requests (overrides benchmark file)").required_unless("benchmark").takes_value(true))
    .arg(Arg::with_name("rampup").short("e").long("rampup").help("Sets the amount of time it takes to reach full concurrency (overrides benchmark file)").takes_value(true))
    .get_matches()
}

struct DrillStats {
  total_requests: usize,
  successful_requests: usize,
  failed_requests: usize,
  mean_duration: f64,
  median_duration: f64,
  stdev_duration: f64,
}

fn compute_stats(sub_reports: &[Report]) -> DrillStats {
  let mut group_by_status = HashMap::new();

  for req in sub_reports {
    group_by_status.entry(req.status / 100).or_insert_with(Vec::new).push(req);
  }

  let mut durations = sub_reports.iter().map(|r| r.duration).collect::<Vec<f64>>();
  let mean_duration = durations.iter().fold(0f64, |a, &b| a + b) / durations.len() as f64;
  let deviations = durations.iter().map(|a| (mean_duration - a).powf(2.0)).collect::<Vec<f64>>();
  let stdev_duration = (deviations.iter().fold(0f64, |a, &b| a + b) / durations.len() as f64).sqrt();

  durations.sort_by(|a, b| a.partial_cmp(b).unwrap());
  let durlen = durations.len();
  let median_duration = if durlen % 2 == 0 {
    durations[durlen / 2]
  } else if durlen > 1 {
    (durations[durlen / 2] + durations[durlen / 2 + 1]) / 2f64
  } else {
    durations[0]
  };

  let total_requests = sub_reports.len();
  let successful_requests = group_by_status.entry(2).or_insert_with(Vec::new).len();
  let failed_requests = total_requests - successful_requests;

  DrillStats {
    total_requests,
    successful_requests,
    failed_requests,
    mean_duration,
    median_duration,
    stdev_duration,
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
    println!("{:width$} {:width2$} {}", name.green(), "Median time per request".yellow(), format_time(substats.median_duration, nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Average time per request".yellow(), format_time(substats.mean_duration, nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Sample standard deviation".yellow(), format_time(substats.stdev_duration, nanosec).purple(), width = 25, width2 = 25);
  }

  // compute global stats
  let allreports = list_reports.concat();
  let global_stats = compute_stats(&allreports);
  let requests_per_second = global_stats.total_requests as f64 / duration;

  println!();
  println!("{:width2$} {} {}", "Time taken for tests".yellow(), format!("{:.1}", duration).purple(), "seconds".purple(), width2 = 25);
  println!("{:width2$} {}", "Total requests".yellow(), global_stats.total_requests.to_string().purple(), width2 = 25);
  println!("{:width2$} {}", "Successful requests".yellow(), global_stats.successful_requests.to_string().purple(), width2 = 25);
  println!("{:width2$} {}", "Failed requests".yellow(), global_stats.failed_requests.to_string().purple(), width2 = 25);
  println!("{:width2$} {} {}", "Requests per second".yellow(), format!("{:.2}", requests_per_second).purple(), "[#/sec]".purple(), width2 = 25);
  println!("{:width2$} {}", "Median time per request".yellow(), format_time(global_stats.median_duration, nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "Average time per request".yellow(), format_time(global_stats.mean_duration, nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "Sample standard deviation".yellow(), format_time(global_stats.stdev_duration, nanosec).purple(), width2 = 25);
}

fn compare_benchmark(list_reports: &[Vec<Report>], compare_path_option: Option<&str>, threshold_option: Option<f64>) {
  if let Some(compare_path) = compare_path_option {
    if let Some(threshold) = threshold_option {
      let compare_result = checker::compare(&list_reports, compare_path, threshold);

      match compare_result {
        Ok(_) => process::exit(0),
        Err(_) => process::exit(1),
      }
    } else {
      panic!("Threshold needed!");
    }
  }
}
