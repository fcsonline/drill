use crate::actions::Report;
use colored::*;
use std::collections::HashMap;
use std::f64;

struct DrillStats {
  total_requests: usize,
  successful_requests: usize,
  failed_requests: usize,
  mean_duration: f64,
  median_duration: f64,
  stdev_duration: f64,
}

pub fn show_stats(list_reports: &Vec<Vec<Report>>, nanosec: bool, duration: f64) {
  let mut group_by_name = HashMap::new();

  for req in list_reports.concat() {
    group_by_name.entry(req.name.clone()).or_insert(Vec::new()).push(req);
  }

  // compute stats per name
  for (name, reports) in group_by_name {
    let substats = compute_stats(&reports);
    println!("");
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

  println!("");
  println!("{:width2$} {}", "Concurrency Level".yellow(), list_reports.len().to_string().purple(), width2 = 25);
  println!("{:width2$} {} {}", "Time taken for tests".yellow(), format!("{:.1}", duration).to_string().purple(), "seconds".purple(), width2 = 25);
  println!("{:width2$} {}", "Total requests".yellow(), global_stats.total_requests.to_string().purple(), width2 = 25);
  println!("{:width2$} {}", "Successful requests".yellow(), global_stats.successful_requests.to_string().purple(), width2 = 25);
  println!("{:width2$} {}", "Failed requests".yellow(), global_stats.failed_requests.to_string().purple(), width2 = 25);
  println!("{:width2$} {} {}", "Requests per second".yellow(), format!("{:.2}", requests_per_second).to_string().purple(), "[#/sec]".purple(), width2 = 25);
  println!("{:width2$} {}", "Median time per request".yellow(), format_time(global_stats.median_duration, nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "Average time per request".yellow(), format_time(global_stats.mean_duration, nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "Sample standard deviation".yellow(), format_time(global_stats.stdev_duration, nanosec).purple(), width2 = 25);
}

fn compute_stats(sub_reports: &Vec<Report>) -> DrillStats {
  let mut group_by_status = HashMap::new();

  for req in sub_reports {
    group_by_status.entry(req.status / 100).or_insert(Vec::new()).push(req);
  }

  let durations = sub_reports.iter().map(|r| r.duration).collect::<Vec<f64>>();
  let mean_duration = durations.iter().fold(0f64, |a, &b| a + b) / durations.len() as f64;
  let deviations = durations.iter().map(|a| (mean_duration - a).powf(2.0)).collect::<Vec<f64>>();
  let stdev_duration = (deviations.iter().fold(0f64, |a, &b| a + b) / durations.len() as f64).sqrt();

  let mut sorted = durations.clone();
  sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
  let durlen = sorted.len();
  let median_duration = if durlen == 0 {
    0.0
  } else if durlen % 2 == 0 {
    sorted[durlen / 2]
  } else if durlen > 1 {
    (sorted[durlen / 2] + sorted[durlen / 2 + 1]) / 2f64
  } else {
    sorted[0]
  };

  let total_requests = sub_reports.len();
  let successful_requests = group_by_status.entry(2).or_insert(Vec::new()).len();
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
