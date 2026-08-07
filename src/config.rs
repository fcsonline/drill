use serde_yaml::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

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
  pub vars: HashMap<String, serde_json::Value>,
  pub threads: usize,
  pub conn_per_iter: bool,
  pub persist_context: bool,
  pub run_time: u64,
  pub continue_on_assert_fail: bool,
  pub success_codes: Vec<u16>,
  pub assertion_failures: Arc<AtomicUsize>,
  pub stats_json: bool,
}

impl Config {
  #[expect(clippy::too_many_arguments, reason = "Config is assembled from CLI flags and YAML; consolidated into a builder as a follow-up")]
  pub fn new(path: &str, relaxed_interpolations: bool, no_check_certificate: bool, quiet: bool, nanosec: bool, timeout: u64, verbose: bool, stats_json: bool) -> Config {
    let config_docs = reader::read_file_as_yml(path);
    let config_doc = &config_docs[0];

    let context: Context = Context::new();
    let interpolator = interpolator::Interpolator::new(&context);

    let iterations = read_i64_configuration(config_doc, &interpolator, "iterations", NITERATIONS);
    let concurrency = read_i64_configuration(config_doc, &interpolator, "concurrency", iterations);
    let rampup = read_i64_configuration(config_doc, &interpolator, "rampup", NRAMPUP);
    let threads = read_usize_configuration(config_doc, &interpolator, "threads", num_cpus::get());
    let conn_per_iter = read_bool_configuration(config_doc, &interpolator, "new_conn_per_iter", false);
    let persist_context = read_bool_configuration(config_doc, &interpolator, "persist_context", false);
    let run_time = read_i64_configuration(config_doc, &interpolator, "run_time", 0).max(0) as u64;
    let success_codes = read_u16_vec_configuration(config_doc, "success_codes");
    let base = read_str_configuration(config_doc, &interpolator, "base", "");
    let results = read_results_configuration(config_doc);
    let lifecycle = read_lifecycle_configuration(config_doc);
    let load_shape = read_load_shape_configuration(config_doc);
    let vars = read_vars_configuration(config_doc);

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
      vars,
      threads,
      conn_per_iter,
      persist_context,
      run_time,
      continue_on_assert_fail: false,
      success_codes,
      assertion_failures: Arc::new(AtomicUsize::new(0)),
      stats_json,
    }
  }

  /// Merge variables coming from a separate `--vars` YAML file into the
  /// config. Values loaded from the file take precedence over the `vars:`
  /// block defined inside the benchmark file.
  pub fn add_vars(&mut self, vars: HashMap<String, serde_json::Value>) {
    self.vars.extend(vars);
  }
}

fn read_vars_configuration(config_doc: &Value) -> HashMap<String, serde_json::Value> {
  let mut vars = HashMap::new();

  if let Some(mapping) = config_doc.get("vars").and_then(|v| v.as_mapping()) {
    for (key, value) in mapping {
      if let Some(key) = key.as_str() {
        vars.insert(key.to_string(), yaml_to_json(value));
      }
    }
  }

  vars
}

/// Load a flat `key: value` mapping from a YAML file into context values.
pub fn parse_vars_file(content: &str) -> HashMap<String, serde_json::Value> {
  let mut vars = HashMap::new();

  match serde_yaml::from_str::<Value>(content) {
    Ok(Value::Mapping(mapping)) => {
      for (key, value) in mapping {
        if let Some(key) = key.as_str() {
          vars.insert(key.to_string(), yaml_to_json(&value));
        } else {
          println!("Invalid variable key: {key:?}");
        }
      }
    }
    Ok(_) => println!("Invalid vars: expected a mapping of key/value pairs"),
    Err(e) => println!("Failed to parse vars file: {e}"),
  }

  vars
}

fn yaml_to_json(value: &Value) -> serde_json::Value {
  match value {
    Value::Null => serde_json::Value::Null,
    Value::Bool(b) => serde_json::Value::Bool(*b),
    Value::Number(n) => {
      if let Some(i) = n.as_i64() {
        serde_json::Value::Number(i.into())
      } else if let Some(u) = n.as_u64() {
        serde_json::Value::Number(u.into())
      } else if let Some(f) = n.as_f64() {
        serde_json::from_str(&f.to_string()).unwrap_or(serde_json::Value::Null)
      } else {
        serde_json::Value::Null
      }
    }
    Value::String(s) => serde_json::Value::String(s.clone()),
    Value::Sequence(seq) => serde_json::Value::Array(seq.iter().map(yaml_to_json).collect()),
    Value::Mapping(map) => {
      let mut obj = serde_json::Map::new();
      for (key, value) in map {
        if let Some(key) = key.as_str() {
          obj.insert(key.to_string(), yaml_to_json(value));
        }
      }
      serde_json::Value::Object(obj)
    }
    _ => serde_json::Value::Null,
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

    parsed.push(LoadShapeStage {
      duration,
      users,
      spawn_rate,
    });
  }

  if parsed.is_empty() {
    None
  } else {
    Some(LoadShapeConfig {
      stages: parsed,
    })
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

fn read_bool_configuration(config_doc: &Value, interpolator: &interpolator::Interpolator, name: &str, default: bool) -> bool {
  let value = if let Some(value) = config_doc.get(name).and_then(|v| v.as_bool()) {
    Some(value)
  } else if let Some(key) = config_doc.get(name).and_then(|v| v.as_str()) {
    interpolator.resolve(key, false).parse::<bool>().ok()
  } else {
    None
  };

  match value {
    Some(value) => value,
    None => {
      if config_doc.get(name).and_then(|v| v.as_str()).is_some() {
        println!("Invalid {name} value!");
      }
      default
    }
  }
}

fn read_usize_configuration(config_doc: &Value, interpolator: &interpolator::Interpolator, name: &str, default: usize) -> usize {
  let value = if let Some(value) = config_doc.get(name).and_then(|v| v.as_i64()) {
    Some(value as usize)
  } else if let Some(key) = config_doc.get(name).and_then(|v| v.as_str()) {
    interpolator.resolve(key, false).parse::<usize>().ok()
  } else {
    None
  };

  match value {
    Some(value) => {
      if value == 0 {
        println!("Invalid zero {name} value!");
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

fn read_u16_vec_configuration(config_doc: &Value, name: &str) -> Vec<u16> {
  match config_doc.get(name) {
    Some(Value::Sequence(seq)) => seq.iter().map(|v| v.as_u64().unwrap_or_else(|| panic!("{name} values must be positive integers, got {v:?}")) as u16).collect(),
    Some(Value::Number(n)) => vec![n.as_u64().unwrap_or_else(|| panic!("{name} must be a positive integer, got {n:?}")) as u16],
    Some(other) => {
      println!("Invalid {name}: expected a number or list of numbers, got {other:?}");
      Vec::new()
    }
    None => Vec::new(),
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
    let config = Config::new(file.path().to_str().unwrap(), false, false, true, false, 10, false, false);

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
    let config = Config::new(file.path().to_str().unwrap(), false, false, true, false, 10, false, false);

    assert!(config.lifecycle.setup.is_some());
    assert!(config.lifecycle.teardown.is_some());
    assert!(config.lifecycle.iteration_start.is_some());
    assert!(config.lifecycle.iteration_stop.is_some());
  }

  #[test]
  fn load_shape_configuration_is_optional() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"---\niterations: 1\nconcurrency: 1\nbase: 'http://localhost'\n").unwrap();
    let config = Config::new(file.path().to_str().unwrap(), false, false, true, false, 10, false, false);

    assert!(config.load_shape.is_none());
  }

  #[test]
  fn load_shape_configuration_is_parsed() {
    let yaml = b"---\niterations: 10\nbase: 'http://localhost'\nload_shape:\n  stages:\n    - duration: 2\n      users: 5\n      spawn_rate: 2\n    - duration: 3\n      users: 10\n";
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(yaml).unwrap();
    let config = Config::new(file.path().to_str().unwrap(), false, false, true, false, 10, false, false);

    let load_shape = config.load_shape.expect("load_shape should be parsed");
    assert_eq!(load_shape.stages.len(), 2);
    assert_eq!(load_shape.stages[0].duration, 2);
    assert_eq!(load_shape.stages[0].users, 5);
    assert_eq!(load_shape.stages[0].spawn_rate, Some(2));
    assert_eq!(load_shape.stages[1].duration, 3);
    assert_eq!(load_shape.stages[1].users, 10);
    assert_eq!(load_shape.stages[1].spawn_rate, None);
  }

  #[test]
  fn vars_block_is_optional() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"---\niterations: 1\nconcurrency: 1\nbase: 'http://localhost'\n").unwrap();
    let config = Config::new(file.path().to_str().unwrap(), false, false, true, false, 10, false, false);

    assert!(config.vars.is_empty());
  }

  #[test]
  fn vars_block_is_parsed() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"---\niterations: 1\nconcurrency: 1\nbase: 'http://localhost'\nvars:\n  api_key: abc123\n  username: john\n").unwrap();
    let config = Config::new(file.path().to_str().unwrap(), false, false, true, false, 10, false, false);

    assert_eq!(config.vars.get("api_key").and_then(|v| v.as_str()), Some("abc123"));
    assert_eq!(config.vars.get("username").and_then(|v| v.as_str()), Some("john"));
  }

  #[test]
  fn parse_vars_file_loads_a_flat_mapping() {
    let vars = super::parse_vars_file("api_key: abc123\ncount: 42\nenabled: true\nnested:\n  a: 1\n");

    assert_eq!(vars.get("api_key").and_then(|v| v.as_str()), Some("abc123"));
    assert_eq!(vars.get("count").and_then(|v| v.as_i64()), Some(42));
    assert_eq!(vars.get("enabled").and_then(|v| v.as_bool()), Some(true));
    assert!(vars.get("nested").and_then(|v| v.as_object()).is_some());
  }

  #[test]
  fn add_vars_merges_with_file_taking_precedence() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"---\niterations: 1\nconcurrency: 1\nbase: 'http://localhost'\nvars:\n  api_key: from-benchmark\n  username: john\n").unwrap();
    let mut config = Config::new(file.path().to_str().unwrap(), false, false, true, false, 10, false, false);

    let mut external = std::collections::HashMap::new();
    external.insert("api_key".to_string(), serde_json::json!("from-file"));

    config.add_vars(external);

    assert_eq!(config.vars.get("api_key").and_then(|v| v.as_str()), Some("from-file"));
    assert_eq!(config.vars.get("username").and_then(|v| v.as_str()), Some("john"));
  }
}
