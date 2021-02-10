use std::convert::TryFrom;

use yaml_rust::{Yaml, YamlLoader};

use crate::benchmark::{BenchmarkOptions, Context};
use crate::interpolator;
use crate::reader;

const NCONCURRENCY: usize = 1;
const NITERATIONS: usize = 1;
const NRAMPUP: usize = 0;

pub struct Config {
  pub base: String,
  pub concurrency: usize,
  pub iterations: usize,
  pub relaxed_interpolations: bool,
  pub no_check_certificate: bool,
  pub rampup: usize,
  pub quiet: bool,
  pub nanosec: bool,
}

impl Config {
  pub fn new(options: &BenchmarkOptions) -> Config {
    let mut config = Config {
      base: "".to_owned(),
      concurrency: NCONCURRENCY,
      iterations: NITERATIONS,
      relaxed_interpolations: options.relaxed_interpolations,
      no_check_certificate: options.no_check_certificate,
      rampup: NRAMPUP,
      quiet: options.quiet,
      nanosec: options.nanosec,
    };
    // load options from benchmark file
    if options.benchmark_path_option.is_some() {
      let config_file = reader::read_file(options.benchmark_path_option.unwrap());

      let config_docs = YamlLoader::load_from_str(config_file.as_str()).unwrap();
      let config_doc = &config_docs[0];

      let context: Context = Context::new();
      let interpolator = interpolator::Interpolator::new(&context);

      if let Some(value) = read_i64_configuration(config_doc, &interpolator, "iterations") {
        config.iterations = usize::try_from(value).expect("Expecting a positive integer value for 'iterations' parameter");
      }
      if let Some(value) = read_i64_configuration(config_doc, &interpolator, "concurrency") {
        config.concurrency = usize::try_from(value).expect("Expecting a positive integer value for 'concurrency' parameter");
      }
      if let Some(value) = read_i64_configuration(config_doc, &interpolator, "rampup") {
        config.rampup = usize::try_from(value).expect("Expecting a positive integer value for 'rampup' parameter");
      }
      if let Some(value) = read_str_configuration(config_doc, &interpolator, "base") {
        config.base = value;
      }
    }
    // overwrite defaults and options from benchmark file with those from command line (BenchmarkOptions struct)
    if let Some(value) = options.base_url_option {
      config.base = value.to_owned();
    }
    if let Some(value) = options.concurrency_option {
      config.concurrency = value;
    }
    if let Some(value) = options.iterations_option {
      config.iterations = value;
    }
    if let Some(value) = options.rampup_option {
      config.rampup = value;
    }

    if config.concurrency > config.iterations {
      panic!("The concurrency can not be higher than the number of iterations")
    }

    return config;
  }
}

fn read_str_configuration(config_doc: &Yaml, interpolator: &interpolator::Interpolator, name: &str) -> Option<String> {
  if let Some(value) = config_doc[name].as_str() {
    if value.contains('{') {
      Some(interpolator.resolve(&value, true).to_owned())
    } else {
      Some(value.to_owned())
    }
  } else {
    if config_doc[name].as_str().is_some() {
      println!("Invalid {} value!", name)
    };
    None
  }
}

// Note: yaml_rust can't parse directly into usize yet, so must be i64 for now
fn read_i64_configuration(config_doc: &Yaml, interpolator: &interpolator::Interpolator, name: &str) -> Option<i64> {
  if let Some(value) = config_doc[name].as_i64() {
    Some(value)
  } else if let Some(key) = config_doc[name].as_str() {
    Some(interpolator.resolve(&key, false).parse::<i64>().expect(format!("Unable to parse benchmark option '{}' into i64", name).as_str()))
  } else {
    if config_doc[name].as_str().is_some() {
      println!("Invalid {} value!", name)
    };
    None
  }
}
