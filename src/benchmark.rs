use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};

use serde_json::{Map, Value, json};
use tokio::{runtime, time::sleep};

use crate::actions::{Report, Runnable};
use crate::config::Config;
use crate::expandable::include;
use crate::metrics::MetricsMiddleware;
use crate::results;
use crate::tags::Tags;
use crate::writer;

use reqwest::ClientBuilder as ReqwestClientBuilder;
use reqwest_middleware::ClientBuilder;

use colored::*;

pub type Benchmark = Vec<Box<dyn Runnable + Sync + Send>>;
pub type Context = Map<String, Value>;
pub type Reports = Vec<Report>;

/// A pooled HTTP client with both the raw `reqwest::Client` (used for request
/// construction methods like `.form()` and `.multipart()`) and the middleware
/// wrapper used for execution and metrics capture.
#[derive(Clone)]
pub struct ClientEntry {
  pub client: reqwest::Client,
  pub middleware: reqwest_middleware::ClientWithMiddleware,
}

pub type PoolStore = HashMap<String, ClientEntry>;
pub type Pool = Arc<Mutex<PoolStore>>;

impl ClientEntry {
  pub fn new(danger_accept_invalid_certs: bool) -> Self {
    let client = ReqwestClientBuilder::default().danger_accept_invalid_certs(danger_accept_invalid_certs).build().unwrap();
    let middleware = ClientBuilder::new(client.clone()).with(MetricsMiddleware::new()).build();
    ClientEntry { client, middleware }
  }
}

pub struct BenchmarkResult {
  pub reports: Vec<Reports>,
  pub duration: f64,
}

pub struct Lifecycle {
  pub setup: Option<Benchmark>,
  pub teardown: Option<Benchmark>,
  pub iteration_start: Option<Benchmark>,
  pub iteration_stop: Option<Benchmark>,
}

async fn run_lifecycle_phase(phase: &Option<Benchmark>, context: &mut Context, reports: &mut Vec<Report>, pool: &Pool, config: &Config) {
  if let Some(benchmark) = phase {
    for item in benchmark.iter() {
      item.execute(context, reports, pool, config).await;
    }
  }
}

fn has_weights(benchmark: &Benchmark) -> bool {
  benchmark.iter().any(|item| item.weight() != 1)
}

async fn run_iteration(benchmark: Arc<Benchmark>, pool: Pool, config: Arc<Config>, iteration: i64, lifecycle: Arc<Lifecycle>, mut context: Context, start_delay: Duration) -> Vec<Report> {
  sleep(start_delay).await;

  let mut reports: Vec<Report> = Vec::new();

  context.insert("iteration".to_string(), json!(iteration.to_string()));

  run_lifecycle_phase(&lifecycle.iteration_start, &mut context, &mut reports, &pool, &config).await;

  if has_weights(&benchmark) {
    use rand::distr::weighted::WeightedIndex;
    use rand::prelude::*;

    let weights: Vec<u32> = benchmark.iter().map(|item| item.weight()).collect();
    let dist = WeightedIndex::new(&weights).expect("Invalid task weights");
    let mut rng = rand::rng();
    let idx = dist.sample(&mut rng);

    benchmark[idx].execute(&mut context, &mut reports, &pool, &config).await;
  } else {
    for item in benchmark.iter() {
      item.execute(&mut context, &mut reports, &pool, &config).await;
    }
  }

  run_lifecycle_phase(&lifecycle.iteration_stop, &mut context, &mut reports, &pool, &config).await;

  reports
}

fn compute_load_shape_schedule(load_shape: &crate::config::LoadShapeConfig, iterations: i64) -> Vec<Duration> {
  let mut users_per_second = Vec::new();
  let mut current_users: i64 = 0;

  for stage in &load_shape.stages {
    let duration = stage.duration as i64;
    let end_users = stage.users as i64;

    for t in 0..duration {
      let target_users = if duration <= 1 {
        end_users
      } else {
        current_users + (end_users - current_users) * (t + 1) / duration
      };
      users_per_second.push(target_users.max(0) as u64);
    }

    current_users = end_users;
  }

  let cumulative: Vec<u64> = users_per_second.iter().scan(0u64, |acc, &u| {
    *acc += u;
    Some(*acc)
  }).collect();

  let total_user_seconds = cumulative.last().copied().unwrap_or(0);
  let iterations = iterations.max(1);

  (0..iterations)
    .map(|i| {
      let target = (i as u64) * total_user_seconds / (iterations as u64);
      let t = cumulative.iter().position(|&c| c >= target).unwrap_or(cumulative.len()) as u64;
      Duration::from_secs(t)
    })
    .collect()
}

fn build_lifecycle(benchmark_path: &str, config: &Config, tags: &Tags) -> Lifecycle {
  let mut lifecycle = Lifecycle {
    setup: None,
    teardown: None,
    iteration_start: None,
    iteration_stop: None,
  };

  if let Some(setup) = config.lifecycle.setup.as_ref().and_then(|v| v.as_sequence()) {
    let mut benchmark = Benchmark::new();
    include::expand_sequence(benchmark_path, setup, &mut benchmark, tags);
    lifecycle.setup = Some(benchmark);
  }

  if let Some(teardown) = config.lifecycle.teardown.as_ref().and_then(|v| v.as_sequence()) {
    let mut benchmark = Benchmark::new();
    include::expand_sequence(benchmark_path, teardown, &mut benchmark, tags);
    lifecycle.teardown = Some(benchmark);
  }

  if let Some(iteration_start) = config.lifecycle.iteration_start.as_ref().and_then(|v| v.as_sequence()) {
    let mut benchmark = Benchmark::new();
    include::expand_sequence(benchmark_path, iteration_start, &mut benchmark, tags);
    lifecycle.iteration_start = Some(benchmark);
  }

  if let Some(iteration_stop) = config.lifecycle.iteration_stop.as_ref().and_then(|v| v.as_sequence()) {
    let mut benchmark = Benchmark::new();
    include::expand_sequence(benchmark_path, iteration_stop, &mut benchmark, tags);
    lifecycle.iteration_stop = Some(benchmark);
  }

  lifecycle
}

fn join<S: ToString>(l: Vec<S>, sep: &str) -> String {
  l.iter().fold(
    "".to_string(),
    |a,b| if !a.is_empty() {a+sep} else {a} + &b.to_string()
  )
}

#[allow(clippy::too_many_arguments)]
pub fn execute(benchmark_path: &str, report_path_option: Option<&str>, relaxed_interpolations: bool, no_check_certificate: bool, quiet: bool, nanosec: bool, timeout: Option<&str>, verbose: bool, tags: &Tags) -> BenchmarkResult {
  let config = Arc::new(Config::new(benchmark_path, relaxed_interpolations, no_check_certificate, quiet, nanosec, timeout.map_or(10, |t| t.parse().unwrap_or(10)), verbose));

  if report_path_option.is_some() {
    println!("{}: {}. Ignoring {} and {} properties...", "Report mode".yellow(), "on".purple(), "concurrency".yellow(), "iterations".yellow());
  } else {
    println!("{} {}", "Concurrency".yellow(), config.concurrency.to_string().purple());
    println!("{} {}", "Iterations".yellow(), config.iterations.to_string().purple());
    println!("{} {}", "Rampup".yellow(), config.rampup.to_string().purple());
  }

  println!("{} {}", "Base URL".yellow(), config.base.purple());
  println!();

  let threads = std::cmp::min(num_cpus::get(), config.concurrency as usize);
  let rt = runtime::Builder::new_current_thread().enable_all().worker_threads(threads).build().unwrap();

  rt.block_on(async {
    let mut benchmark: Benchmark = Benchmark::new();
    let pool_store: PoolStore = PoolStore::new();

    include::expand_from_filepath(benchmark_path, &mut benchmark, Some("plan"), tags);

    if benchmark.is_empty() {
      eprintln!("Empty benchmark. Exiting.");
      std::process::exit(1);
    }

    let benchmark = Arc::new(benchmark);
    let pool = Arc::new(Mutex::new(pool_store));
    let lifecycle = Arc::new(build_lifecycle(benchmark_path, &config, tags));

    let mut setup_context = Context::new();
    setup_context.insert("base".to_string(), json!(config.base.to_string()));

    if let Some(report_path) = report_path_option {
      let mut setup_reports = Vec::new();
      run_lifecycle_phase(&lifecycle.setup, &mut setup_context, &mut setup_reports, &pool, &config).await;

      let iteration_context = setup_context.clone();
      let mut reports = run_iteration(benchmark.clone(), pool.clone(), config.clone(), 0, lifecycle.clone(), iteration_context, Duration::ZERO).await;

      run_lifecycle_phase(&lifecycle.teardown, &mut setup_context, &mut reports, &pool, &config).await;
      reports.extend(setup_reports);

      writer::write_file(report_path, join(reports, ""));

      BenchmarkResult {
        reports: vec![],
        duration: 0.0,
      }
    } else {
      let mut setup_reports = Vec::new();
      run_lifecycle_phase(&lifecycle.setup, &mut setup_context, &mut setup_reports, &pool, &config).await;

      let start_delays = if let Some(load_shape) = config.load_shape.as_ref() {
        compute_load_shape_schedule(load_shape, config.iterations)
      } else {
        (0..config.iterations)
          .map(|iteration| {
            if config.rampup > 0 {
              let delay = config.rampup / config.iterations;
              Duration::new((delay * iteration) as u64, 0)
            } else {
              Duration::ZERO
            }
          })
          .collect()
      };

      let max_concurrency = if let Some(load_shape) = config.load_shape.as_ref() {
        load_shape.stages.iter().map(|s| s.users).max().unwrap_or(1) as usize
      } else {
        config.concurrency as usize
      };

      let base_context = setup_context.clone();
      let children = (0..config.iterations).map(|iteration| {
        let start_delay = start_delays[iteration as usize];
        run_iteration(benchmark.clone(), pool.clone(), config.clone(), iteration, lifecycle.clone(), base_context.clone(), start_delay)
      });

      let buffered = stream::iter(children).buffer_unordered(max_concurrency);

      let begin = Instant::now();
      let mut reports: Vec<Vec<Report>> = buffered.collect::<Vec<_>>().await;
      let duration = begin.elapsed().as_secs_f64();

      let mut teardown_reports = Vec::new();
      run_lifecycle_phase(&lifecycle.teardown, &mut setup_context, &mut teardown_reports, &pool, &config).await;

      reports.push(setup_reports);
      reports.push(teardown_reports);

      if let Some(results_config) = config.results.as_ref() {
        results::generate(&reports, duration, results_config);
      }

      BenchmarkResult {
        reports,
        duration,
      }
    }
  })
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use serde_yaml::Value;

  use crate::benchmark::{Benchmark, has_weights};
  use crate::actions::Assign;

  fn yaml(text: &str) -> Value {
    let docs = crate::reader::read_file_as_yml_from_str(text);
    docs[0].clone()
  }

  #[test]
  fn has_weights_false_when_all_default() {
    let benchmark: Benchmark = vec![
      Box::new(Assign::new(&yaml("---\nname: A\nassign:\n  key: a\n  value: '1'"), None)),
      Box::new(Assign::new(&yaml("---\nname: B\nassign:\n  key: b\n  value: '2'"), None)),
    ];

    assert!(!has_weights(&benchmark));
  }

  #[test]
  fn has_weights_true_when_any_differ() {
    let benchmark: Benchmark = vec![
      Box::new(Assign::new(&yaml("---\nname: A\nassign:\n  key: a\n  value: '1'\nweight: 3"), None)),
      Box::new(Assign::new(&yaml("---\nname: B\nassign:\n  key: b\n  value: '2'"), None)),
    ];

    assert!(has_weights(&benchmark));
  }

  #[test]
  fn load_shape_schedule_spaces_iterations_by_user_seconds() {
    use crate::config::{LoadShapeConfig, LoadShapeStage};

    let load_shape = LoadShapeConfig {
      stages: vec![
        LoadShapeStage { duration: 2, users: 10, spawn_rate: None },
        LoadShapeStage { duration: 2, users: 10, spawn_rate: None },
      ],
    };

    let schedule = super::compute_load_shape_schedule(&load_shape, 4);

    assert_eq!(schedule.len(), 4);
    assert_eq!(schedule[0], Duration::from_secs(0));
    assert_eq!(schedule[3], Duration::from_secs(3));
  }
}
