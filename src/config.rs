use serde_yaml::Value;

use crate::benchmark::Context;
use crate::interpolator;
use crate::reader;

const NITERATIONS: i64 = 1;
const NRAMPUP: i64 = 0;

#[derive(Clone)]
pub struct ResultsConfig {
  pub output_dir: String,
  pub csv: bool,
  pub html: bool,
}

impl Default for ResultsConfig {
  fn default() -> Self {
    ResultsConfig {
      output_dir: "drill-results".to_string(),
      csv: true,
      html: true,
    }
  }
}

pub struct Config {
  pub base: String,
  pub concurrency: i64,
  pub iterations: i64,
  pub relaxed_interpolations: bool,
  pub no_check_certificate: bool,
  pub rampup: i64,
  pub quiet: bool,
  pub nanosec: bool,
  pub timeout: u64,
  pub verbose: bool,
  pub results: Option<ResultsConfig>,
}

impl Config {
  pub fn new(path: &str, relaxed_interpolations: bool, no_check_certificate: bool, quiet: bool, nanosec: bool, timeout: u64, verbose: bool) -> Config {
    let config_docs = reader::read_file_as_yml(path);
    let config_doc = &config_docs[0];

    let context: Context = Context::new();
    let interpolator = interpolator::Interpolator::new(&context);

    let iterations = read_i64_configuration(config_doc, &interpolator, "iterations", NITERATIONS);
    let concurrency = read_i64_configuration(config_doc, &interpolator, "concurrency", iterations);
    let rampup = read_i64_configuration(config_doc, &interpolator, "rampup", NRAMPUP);
    let base = read_str_configuration(config_doc, &interpolator, "base", "");
    let results = read_results_configuration(config_doc);

    if concurrency > iterations {
      panic!("The concurrency can not be higher than the number of iterations")
    }

    Config {
      base,
      concurrency,
      iterations,
      relaxed_interpolations,
      no_check_certificate,
      rampup,
      quiet,
      nanosec,
      timeout,
      verbose,
      results,
    }
  }
}

fn read_results_configuration(config_doc: &Value) -> Option<ResultsConfig> {
  let value = config_doc.get("results")?;

  let mut config = ResultsConfig::default();

  if let Some(dir) = value.as_str() {
    config.output_dir = dir.to_string();
    return Some(config);
  }

  if let Some(mapping) = value.as_mapping() {
    if let Some(dir) = mapping.get("output_dir").and_then(|v| v.as_str()) {
      config.output_dir = dir.to_string();
    }

    if let Some(csv) = mapping.get("csv").and_then(|v| v.as_bool()) {
      config.csv = csv;
    }

    if let Some(html) = mapping.get("html").and_then(|v| v.as_bool()) {
      config.html = html;
    }

    return Some(config);
  }

  None
}

fn read_str_configuration(config_doc: &Value, interpolator: &interpolator::Interpolator, name: &str, default: &str) -> String {
  match config_doc.get(name).and_then(|v| v.as_str()) {
    Some(value) => {
      if value.contains('{') {
        interpolator.resolve(value, true)
      } else {
        value.to_owned()
      }
    }
    None => {
      if config_doc.get(name).and_then(|v| v.as_str()).is_some() {
        println!("Invalid {name} value!");
      }

      default.to_owned()
    }
  }
}

fn read_i64_configuration(config_doc: &Value, interpolator: &interpolator::Interpolator, name: &str, default: i64) -> i64 {
  let value = if let Some(value) = config_doc.get(name).and_then(|v| v.as_i64()) {
    Some(value)
  } else if let Some(key) = config_doc.get(name).and_then(|v| v.as_str()) {
    interpolator.resolve(key, false).parse::<i64>().ok()
  } else {
    None
  };

  match value {
    Some(value) => {
      if value < 0 {
        println!("Invalid negative {name} value!");

        default
      } else {
        value
      }
    }
    None => {
      if config_doc.get(name).and_then(|v| v.as_str()).is_some() {
        println!("Invalid {name} value!");
      }

      default
    }
  }
}
