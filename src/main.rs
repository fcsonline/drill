mod actions;
mod benchmark;
mod checker;
mod cli;
mod config;
mod expandable;
mod interpolator;
mod parser;
mod reader;
mod tags;
mod writer;

use crate::actions::Report;
use clap::Parser;
use colored::*;
use hashlink::LinkedHashMap;
use hdrhistogram::Histogram;
use std::collections::HashMap;
use std::{io, process};

fn main() {
    let args = cli::Args::parse();

    #[cfg(windows)]
    let _ = control::set_virtual_terminal(true);

    if args.list_tags {
        match tags::list_benchmark_file_tags(args.benchmark.as_str()) {
            Err(err) => {
                eprintln!("{err}");
                process::exit(1);
            }
            Ok(_) => (),
        };
        process::exit(0);
    };

    let tags = match tags::Tags::new(args.tags.as_deref(), args.skip_tags.as_deref()) {
        Ok(tags) => tags,
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    };

    if args.list_tasks {
        match tags::list_benchmark_file_tasks(args.benchmark.as_str(), &tags) {
            Err(err) => {
                eprintln!("{err}");
                process::exit(1);
            }
            Ok(_) => (),
        };
        process::exit(0);
    };

    let benchmark_result = benchmark::execute(args.benchmark.as_str(), args.report.as_deref(), args.relaxed_interpolations, args.no_check_certificate, args.quiet, args.nanosec, args.timeout, args.verbose, &tags);

    let list_reports: Vec<Vec<Report>>;
    let duration: f64;

    match benchmark_result {
        Ok(result) => {
            list_reports = result.reports;
            duration = result.duration;
        }
        Err(err) => {
            eprintln!("{err}");
            process::exit(1)
        }
    };

    show_stats(&list_reports, args.stats, args.nanosec, duration);
    if let Err(err) = compare_benchmark(&list_reports, args.compare.as_deref(), args.threshold) {
        eprintln!("{err}");
        process::exit(1)
    }

    process::exit(0)
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
}

fn compute_stats(sub_reports: &[Report]) -> DrillStats {
    let mut hist = Histogram::<u64>::new_with_bounds(1, 60 * 60 * 1000, 2).unwrap();
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
}

fn compare_benchmark(list_reports: &[Vec<Report>], compare_path_option: Option<&str>, threshold_option: Option<f64>) -> Result<(), io::Error> {
    if let Some(compare_path) = compare_path_option {
        if let Some(threshold) = threshold_option {
            let compare_result = checker::compare(list_reports, compare_path, threshold);

            match compare_result {
                Ok(_) => process::exit(0),
                Err(_) => process::exit(1),
            }
        } else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "threshold needed!"));
        }
    }

    Ok(())
}
