extern crate clap;
extern crate colored;
extern crate csv;
extern crate hyper;
extern crate hyper_tls;
extern crate regex;
extern crate serde_json;
extern crate time;
extern crate yaml_rust;

mod actions;
mod benchmark;
mod checker;
mod config;
mod expandable;
mod interpolator;
mod iteration;
mod reader;
mod stats;
mod writer;

use self::clap::{App, Arg};
use crate::actions::Report;
use clap::crate_version;
use std::process;

fn main() {
  let matches = app_args();
  let benchmark_file = matches.value_of("benchmark").unwrap();
  let report_path_option = matches.value_of("report");
  let stats_option = matches.is_present("stats");
  let compare_path_option = matches.value_of("compare");
  let threshold_option = matches.value_of("threshold");
  let parallel = matches.is_present("parallel");
  let extreme = matches.is_present("extreme");
  let no_check_certificate = matches.is_present("no-check-certificate");
  let quiet = matches.is_present("quiet");
  let nanosec = matches.is_present("nanosec");

  let begin = time::precise_time_s();
  let list_reports_result = benchmark::execute(benchmark_file, report_path_option, no_check_certificate, quiet, nanosec, parallel, extreme);
  let duration = time::precise_time_s() - begin;

  match list_reports_result {
    Ok(list_reports) => {
      if stats_option {
        stats::show_stats(&list_reports, nanosec, duration);
      }

      compare_benchmark(&list_reports, compare_path_option, threshold_option);

      process::exit(0)
    }
    Err(_) => process::exit(1),
  }
}

fn app_args<'a>() -> clap::ArgMatches<'a> {
  return App::new("drill")
    .version(crate_version!())
    .about("HTTP load testing application written in Rust inspired by Ansible syntax")
    .arg(Arg::with_name("benchmark").help("Sets the benchmark file").long("benchmark").short("b").required(true).takes_value(true))
    .arg(Arg::with_name("stats").short("s").long("stats").help("Shows request statistics").takes_value(false).conflicts_with("compare"))
    .arg(Arg::with_name("report").short("r").long("report").help("Sets a report file").takes_value(true).conflicts_with("compare"))
    .arg(Arg::with_name("compare").short("c").long("compare").help("Sets a compare file").takes_value(true).conflicts_with("report"))
    .arg(Arg::with_name("threshold").short("t").long("threshold").help("Sets a threshold value in ms amongst the compared file").takes_value(true).conflicts_with("report"))
    .arg(Arg::with_name("parallel").short("p").long("parallel").help("Executes the plan running all iterations in parallel.").takes_value(false).conflicts_with("report").conflicts_with("extreme"))
    .arg(
      Arg::with_name("extreme")
        .short("x")
        .long("extreme")
        .help("Executes the plan at maximum throughput running all items in parallel. Interpolations, sessions and request dependencies are not allowed in this mode")
        .takes_value(false)
        .conflicts_with("report")
        .conflicts_with("parallel"),
    )
    .arg(Arg::with_name("no-check-certificate").long("no-check-certificate").help("Disables SSL certification check. (Not recommended)").takes_value(false))
    .arg(Arg::with_name("quiet").short("q").long("quiet").help("Disables output").takes_value(false))
    .arg(Arg::with_name("nanosec").short("n").long("nanosec").help("Shows statistics in nanoseconds").takes_value(false))
    .get_matches();
}

fn compare_benchmark(list_reports: &Vec<Vec<Report>>, compare_path_option: Option<&str>, threshold_option: Option<&str>) {
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
