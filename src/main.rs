extern crate colored;
extern crate yaml_rust;
extern crate hyper;
extern crate hyper_native_tls;
extern crate time;
extern crate csv;
extern crate regex;
extern crate clap;
extern crate serde_json;

mod config;
mod interpolator;
mod benchmark;
mod reader;
mod checker;
mod writer;
mod actions;
mod expandable;

use self::clap::{Arg, App};
use colored::*;
use std::process;
use std::collections::HashMap;
use std::f64;

fn main() {

  let matches = App::new("drill")
                      .version("0.3.5")
                      .about("HTTP load testing application written in Rust inspired by Ansible syntax")
                      .arg(Arg::with_name("benchmark")
                                  .help("Sets the benchmark file")
                                  .long("benchmark")
                                  .short("b")
                                  .required(true)
                                  .takes_value(true))
                      .arg(Arg::with_name("stats")
                                  .short("s")
                                  .long("stats")
                                  .help("Shows request statistics")
                                  .takes_value(false)
                                  .conflicts_with("compare"))
                      .arg(Arg::with_name("report")
                                  .short("r")
                                  .long("report")
                                  .help("Sets a report file")
                                  .takes_value(true)
                                  .conflicts_with("compare"))
                      .arg(Arg::with_name("compare")
                                  .short("c")
                                  .long("compare")
                                  .help("Sets a compare file")
                                  .takes_value(true)
                                  .conflicts_with("report"))
                      .arg(Arg::with_name("threshold")
                                  .short("t")
                                  .long("threshold")
                                  .help("Sets a threshold value in ms amongst the compared file")
                                  .takes_value(true)
                                  .conflicts_with("report"))
                      .get_matches();

  let benchmark_file = matches.value_of("benchmark").unwrap();
  let report_path_option = matches.value_of("report");
  let stats_option = matches.is_present("stats");
  let compare_path_option = matches.value_of("compare");
  let threshold_option = matches.value_of("threshold");

  let begin = time::precise_time_s();
  let list_reports_result = benchmark::execute(benchmark_file, report_path_option);
  let duration = time::precise_time_s() - begin;

  match list_reports_result {
    Ok(list_reports) => {

      if stats_option {
        let mut group_by_status = HashMap::new();

        for req in list_reports.concat() {
          group_by_status.entry(req.status / 100).or_insert(Vec::new()).push(req);
        }

        let durations = list_reports.concat().iter().map(|r| r.duration).collect::<Vec<f64>>();
        let mean = durations.iter().fold(0f64, |a, &b| a + b) / durations.len() as f64;
        let deviations = durations.iter().map(|a| (mean - a).powf(2.0)).collect::<Vec<f64>>();
        let stdev = (deviations.iter().fold(0f64, |a, &b| a + b) / durations.len() as f64).sqrt();

        let mut sorted = durations.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let durlen = sorted.len();
        let median = if durlen % 2 == 0 {
          sorted[durlen / 2]
        } else {
          (sorted[durlen / 2] + sorted[durlen / 2 + 1]) / 2f64
        };

        let total_requests = list_reports.concat().len();
        let successful_requests = group_by_status.entry(2).or_insert(Vec::new()).len();
        let failed_requests = total_requests - successful_requests;
        let requests_per_second = total_requests as f64 / duration;

        println!("");
        println!("{} {}", "Concurrency Level".yellow(), list_reports.len().to_string().purple());
        println!("{} {} {}", "Time taken for tests".yellow(), format!("{:.1}", duration).to_string().purple(), "seconds".purple());
        println!("{} {}", "Total requests".yellow(), total_requests.to_string().purple());
        println!("{} {}", "Successful requests".yellow(), successful_requests.to_string().purple());
        println!("{} {}", "Failed requests".yellow(), failed_requests.to_string().purple());
        println!("{} {} {}", "Requests per second".yellow(), format!("{:.3}", requests_per_second).to_string().purple(), "[#/sec]".purple());
        println!("{} {}{}", "Median time per request".yellow(), median.round().to_string().purple(), "ms".purple());
        println!("{} {}{}", "Average time per request".yellow(), mean.round().to_string().purple(), "ms".purple());
        println!("{} {}{}", "Sample standard deviation".yellow(), stdev.round().to_string().purple(), "ms".purple());

      }

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

      process::exit(0)
    },
    Err(_) => process::exit(1),
  }
}
