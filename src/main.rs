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
use std::process;

fn main() {

  let matches = App::new("woodpecker")
                      .version("0.1.0")
                      .about("HTTP load testing application written in Rust inspired by Ansible syntax")
                      .arg(Arg::with_name("benchmark")
                                  .help("Sets the benchmark file")
                                  .long("benchmark")
                                  .short("b")
                                  .required(true)
                                  .takes_value(true))
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
  let compare_path_option = matches.value_of("compare");
  let threshold_option = matches.value_of("threshold");

  let list_reports_result = benchmark::execute(benchmark_file, report_path_option);

  match list_reports_result {
    Ok(list_reports) => {

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
