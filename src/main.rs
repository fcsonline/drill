extern crate colored;
extern crate yaml_rust;
extern crate hyper;
extern crate hyper_native_tls;
extern crate time;
extern crate serde_json;
extern crate csv;
extern crate regex;
extern crate clap;

mod config;
mod interpolator;
mod benchmark;
mod reader;
mod actions;
mod expandable;

use self::clap::{Arg, App};

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
                      //.arg(Arg::with_name("compare")
                      //            .short("c")
                      //            .long("compare")
                      //            .help("Sets a compare file")
                      //            .takes_value(true))
                      .get_matches();

  let benchmark_file = matches.value_of("benchmark").unwrap();

  benchmark::execute(benchmark_file);
}
