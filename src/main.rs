extern crate colored;
extern crate yaml_rust;
extern crate hyper;
extern crate hyper_native_tls;
extern crate time;
extern crate serde_json;
extern crate csv;
extern crate regex;

use self::colored::*;

mod config;
mod interpolator;
mod benchmark;
mod reader;
mod actions;
mod expandable;

fn main() {
  let config = config::Config::new("./config.yml");

  println!("{} {}", "Threads".yellow(), config.threads.to_string().purple());
  println!("{} {}", "Iterations".yellow(), config.iterations.to_string().purple());
  println!("{} {}", "Base URL".yellow(), config.base.to_string().purple());
  println!("");

  benchmark::execute("./benchmark.yml", config.threads, config.iterations, config.base);
}
