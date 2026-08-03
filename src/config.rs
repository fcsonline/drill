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

#[derive(Clone, Default)]
pub struct LifecycleConfig {
  pub setup: Option<Value>,
  pub teardown: Option<Value>,
  pub iteration_start: Option<Value>,
  pub iteration_stop: Option<Value>,
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
  pub lifecycle: LifecycleConfig,
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
    let lifecycle = read_lifecycle_configuration(config_doc);

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
      lifecycle,
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

fn read_lifecycle_configuration(config_doc: &Value) -> LifecycleConfig {
  let mut lifecycle = LifecycleConfig::default();

  if let Some(mapping) = config_doc.get("lifecycle").and_then(|v| v.as_mapping()) {
    if let Some(value) = mapping.get("setup").cloned() {
      lifecycle.setup = Some(value);
    }
    if let Some(value) = mapping.get("teardown").cloned() {
      lifecycle.teardown = Some(value);
    }
    if let Some(value) = mapping.get("iteration_start").cloned() {
      lifecycle.iteration_start = Some(value);
    }
    if let Some(value) = mapping.get("iteration_stop").cloned() {
      lifecycle.iteration_stop = Some(value);
    }
  }

  lifecycle
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

#[cfg(test)]
mod tests {
  use std::io::Write;
  use tempfile::NamedTempFile;

  use super::Config;

  #[test]
  fn lifecycle_configuration_is_optional() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"---\niterations: 1\nconcurrency: 1\nbase: 'http://localhost'\n").unwrap();
    let config = Config::new(file.path().to_str().unwrap(), false, false, true, false, 10, false);

    assert!(config.lifecycle.setup.is_none());
    assert!(config.lifecycle.teardown.is_none());
    assert!(config.lifecycle.iteration_start.is_none());
    assert!(config.lifecycle.iteration_stop.is_none());
  }

  #[test]
  fn lifecycle_configuration_is_parsed() {
    let yaml = b"---\niterations: 1\nconcurrency: 1\nbase: 'http://localhost'\nlifecycle:\n  setup:\n    - name: Setup\n      assign:\n        key: setup\n        value: '1'\n  teardown:\n    - name: Teardown\n      assign:\n        key: teardown\n        value: '1'\n  iteration_start:\n    - name: Iteration Start\n      assign:\n        key: iteration_start\n        value: '1'\n  iteration_stop:\n    - name: Iteration Stop\n      assign:\n        key: iteration_stop\n        value: '1'\n";
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(yaml).unwrap();
    let config = Config::new(file.path().to_str().unwrap(), false, false, true, false, 10, false);

    assert!(config.lifecycle.setup.is_some());
    assert!(config.lifecycle.teardown.is_some());
    assert!(config.lifecycle.iteration_start.is_some());
    assert!(config.lifecycle.iteration_stop.is_some());
  }
}
