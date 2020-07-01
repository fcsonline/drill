use yaml_rust::{Yaml, YamlLoader};

use crate::benchmark::Context;
use crate::interpolator;
use crate::reader;

const NCONCURRENCY: i64 = 1;
const NITERATIONS: i64 = 1;
const NRAMPUP: i64 = 0;

pub struct Config {
  pub base: String,
  pub concurrency: i64,
  pub threads: usize,
  pub iterations: i64,
  pub relaxed_interpolations: bool,
  pub no_check_certificate: bool,
  pub rampup: i64,
  pub quiet: bool,
  pub nanosec: bool,
}

impl Config {
  pub fn new(path: &str, relaxed_interpolations: bool, no_check_certificate: bool, quiet: bool, nanosec: bool) -> Config {
    let config_file = reader::read_file(path);

    let config_docs = YamlLoader::load_from_str(config_file.as_str()).unwrap();
    let config_doc = &config_docs[0];

    let context: Context = Context::new();
    let interpolator = interpolator::Interpolator::new(&context);

    let concurrency = read_i64_configuration(config_doc, &interpolator, "concurrency", NCONCURRENCY);
    let threads = std::cmp::min(num_cpus::get(), concurrency as usize);
    let iterations = read_i64_configuration(config_doc, &interpolator, "iterations", NITERATIONS);
    let rampup = read_i64_configuration(config_doc, &interpolator, "rampup", NRAMPUP);
    let base = read_str_configuration(config_doc, &interpolator, "base", "");

    Config {
      base,
      concurrency,
      threads,
      iterations,
      relaxed_interpolations,
      no_check_certificate,
      rampup,
      quiet,
      nanosec,
    }
  }
}

fn read_str_configuration(config_doc: &Yaml, interpolator: &interpolator::Interpolator, name: &str, default: &str) -> String {
  match config_doc[name].as_str() {
    Some(value) => {
      if value.contains('{') {
        interpolator.resolve(&value, true).to_owned()
      } else {
        value.to_owned()
      }
    }
    None => {
      if config_doc[name].as_str().is_some() {
        println!("Invalid {} value!", name);
      }

      default.to_owned()
    }
  }
}

fn read_i64_configuration(config_doc: &Yaml, interpolator: &interpolator::Interpolator, name: &str, default: i64) -> i64 {
  let value = if let Some(value) = config_doc[name].as_i64() {
    Some(value)
  } else if let Some(key) = config_doc[name].as_str() {
    interpolator.resolve(&key, false).parse::<i64>().ok()
  } else {
    None
  };

  match value {
    Some(value) => {
      if value < 0 {
        println!("Invalid negative {} value!", name);

        default
      } else {
        value
      }
    }
    None => {
      if config_doc[name].as_str().is_some() {
        println!("Invalid {} value!", name);
      }

      default
    }
  }
}
