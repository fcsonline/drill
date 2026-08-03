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

#[derive(Clone, Debug)]
pub struct LoadShapeStage {
  pub duration: u64,
  pub users: u64,
  // Reserved for future enforcement; the current scheduler uses the target
  // users per stage as the concurrency limit.
  #[allow(dead_code)]
  pub spawn_rate: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct LoadShapeConfig {
  pub stages: Vec<LoadShapeStage>,
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
  pub load_shape: Option<LoadShapeConfig>,
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
    let load_shape = read_load_shape_configuration(config_doc);

    if concurrency > iterations && load_shape.is_none() {
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
      load_shape,
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

fn read_load_shape_configuration(config_doc: &Value) -> Option<LoadShapeConfig> {
  let stages = config_doc.get("load_shape").and_then(|v| v.get("stages")).and_then(|v| v.as_sequence())?;

  let mut parsed = Vec::new();

  for stage in stages {
    let duration = stage.get("duration").and_then(|v| v.as_u64()).expect("load_shape stage requires a duration");
    let users = stage.get("users").and_then(|v| v.as_u64()).expect("load_shape stage requires a users count");
    let spawn_rate = stage.get("spawn_rate").and_then(|v| v.as_u64());

    parsed.push(LoadShapeStage { duration, users, spawn_rate });
  }

  if parsed.is_empty() {
    None
  } else {
    Some(LoadShapeConfig { stages: parsed })
  }
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

  #[test]
  fn load_shape_configuration_is_optional() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"---\niterations: 1\nconcurrency: 1\nbase: 'http://localhost'\n").unwrap();
    let config = Config::new(file.path().to_str().unwrap(), false, false, true, false, 10, false);

    assert!(config.load_shape.is_none());
  }

  #[test]
  fn load_shape_configuration_is_parsed() {
    let yaml = b"---\niterations: 10\nbase: 'http://localhost'\nload_shape:\n  stages:\n    - duration: 2\n      users: 5\n      spawn_rate: 2\n    - duration: 3\n      users: 10\n";
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(yaml).unwrap();
    let config = Config::new(file.path().to_str().unwrap(), false, false, true, false, 10, false);

    let load_shape = config.load_shape.expect("load_shape should be parsed");
    assert_eq!(load_shape.stages.len(), 2);
    assert_eq!(load_shape.stages[0].duration, 2);
    assert_eq!(load_shape.stages[0].users, 5);
    assert_eq!(load_shape.stages[0].spawn_rate, Some(2));
    assert_eq!(load_shape.stages[1].duration, 3);
    assert_eq!(load_shape.stages[1].users, 10);
    assert_eq!(load_shape.stages[1].spawn_rate, None);
  }
}
