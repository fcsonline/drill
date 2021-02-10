use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};

use serde_json::{json, Value};
use tokio::{runtime, time::delay_for};
use yaml_rust::{yaml, Yaml};

use crate::actions::{Report, Request, Runnable};
use crate::config::Config;
use crate::expandable::include;
use crate::writer;

use reqwest::Client;

use colored::*;

pub type Benchmark = Vec<Box<(dyn Runnable + Sync + Send)>>;
pub type Context = HashMap<String, Value>;
pub type Reports = Vec<Report>;
pub type PoolStore = HashMap<String, Client>;
pub type Pool = Arc<Mutex<PoolStore>>;

// represents un-validated user inputs
pub struct BenchmarkOptions<'a> {
  pub benchmark_path_option: Option<&'a str>,
  pub report_path_option: Option<&'a str>,
  pub relaxed_interpolations: bool,
  pub no_check_certificate: bool,
  pub stats: bool,
  pub compare_path_option: Option<&'a str>,
  pub threshold_option: Option<f64>,
  pub quiet: bool,
  pub nanosec: bool,
  pub concurrency_option: Option<usize>,
  pub iterations_option: Option<usize>,
  pub base_url_option: Option<&'a str>,
  pub rampup_option: Option<usize>,
}

pub struct BenchmarkResult {
  pub reports: Vec<Reports>,
  pub duration: f64,
}

async fn run_iteration(benchmark: Arc<Benchmark>, pool: Pool, config: Arc<Config>, iteration: usize) -> Vec<Report> {
  if config.rampup > 0 {
    let delay = config.rampup / config.iterations;
    delay_for(Duration::new((delay * iteration) as u64, 0)).await;
  }

  let mut context: Context = Context::new();
  let mut reports: Vec<Report> = Vec::new();

  context.insert("iteration".to_string(), json!(iteration.to_string()));
  context.insert("base".to_string(), json!(config.base.to_string()));

  for item in benchmark.iter() {
    item.execute(&mut context, &mut reports, &pool, &config).await;
  }

  reports
}

fn join<S: ToString>(l: Vec<S>, sep: &str) -> String {
  l.iter().fold(
    "".to_string(),
    |a,b| if !a.is_empty() {a+sep} else {a} + &b.to_string()
  )
}

pub fn execute(options: &BenchmarkOptions) -> BenchmarkResult {
  // prepare config
  let config = Arc::new(Config::new(&options));

  if options.report_path_option.is_some() {
    println!("{}: {}. Ignoring {} and {} properties...", "Report mode".yellow(), "on".purple(), "concurrency".yellow(), "iterations".yellow());
  } else {
    println!("{} {}", "Concurrency".yellow(), config.concurrency.to_string().purple());
    println!("{} {}", "Iterations".yellow(), config.iterations.to_string().purple());
    println!("{} {}", "Rampup".yellow(), config.rampup.to_string().purple());
  }

  println!("{} {}", "Base URL".yellow(), config.base.purple());
  println!();

  let threads = std::cmp::min(num_cpus::get(), config.concurrency as usize);
  let mut rt = runtime::Builder::new().threaded_scheduler().enable_all().core_threads(threads).max_threads(threads).build().unwrap();
  rt.block_on(async {
    let mut benchmark: Benchmark = Benchmark::new();
    let pool_store: PoolStore = PoolStore::new();

    if let Some(benchmark_path) = options.benchmark_path_option {
      include::expand_from_filepath(benchmark_path, &mut benchmark, Some("plan"));
    } else {
      // if no benchmark plan is provided. then default to requesting the baseUrl itself.
      let mut default_item = yaml::Hash::new();
      default_item.insert(Yaml::from_str("name"), Yaml::from_str("Default"));
      let mut url_item = yaml::Hash::new();
      url_item.insert(Yaml::from_str("url"), Yaml::from_str("/"));
      default_item.insert(Yaml::from_str("request"), Yaml::Hash(url_item));
      benchmark.push(Box::new(Request::new(&Yaml::Hash(default_item), None, None)));
    }

    let benchmark = Arc::new(benchmark);
    let pool = Arc::new(Mutex::new(pool_store));

    if let Some(report_path) = options.report_path_option {
      let reports = run_iteration(benchmark.clone(), pool.clone(), config, 0).await;

      writer::write_file(report_path, join(reports, ""));

      BenchmarkResult {
        reports: vec![],
        duration: 0.0,
      }
    } else {
      let children = (0..config.iterations).map(|iteration| run_iteration(benchmark.clone(), pool.clone(), config.clone(), iteration));

      let buffered = stream::iter(children).buffer_unordered(config.concurrency as usize);

      let begin = Instant::now();
      let reports: Vec<Vec<Report>> = buffered.collect::<Vec<_>>().await;
      let duration = begin.elapsed().as_secs_f64();

      BenchmarkResult {
        reports,
        duration,
      }
    }
  })
}
