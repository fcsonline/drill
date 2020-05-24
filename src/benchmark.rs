use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};

use serde_json::{json, Value};
use tokio::{runtime, time::delay_for};

use crate::actions::{Report, Runnable};
use crate::config::Config;
use crate::expandable::include;
use crate::writer;

use reqwest::Client;

use colored::*;

pub type Benchmark = Vec<Box<(dyn Runnable + Sync + Send)>>;
pub type Context = HashMap<String, Value>;
pub type Reports = Vec<Report>;
pub type Pool = HashMap<String, Client>;

pub struct BenchmarkResult {
  pub reports: Vec<Reports>,
  pub duration: f64,
}

async fn run_iteration(benchmark: Arc<Benchmark>, config: Arc<Config>, iteration: i64) -> Vec<Report> {
  let delay = config.rampup / config.iterations;
  delay_for(Duration::new((delay * iteration) as u64, 0)).await;

  let mut context: Context = Context::new();
  let mut reports: Vec<Report> = Vec::new();

  let mut pool: Pool = Pool::new(); // TODO: Share pool between all iterations

  context.insert("iteration".to_string(), json!(iteration.to_string()));
  context.insert("base".to_string(), json!(config.base.to_string()));

  for item in benchmark.iter() {
    item.execute(&mut context, &mut reports, &mut pool, &config).await;
  }

  reports
}

fn join<S: ToString>(l: Vec<S>, sep: &str) -> String {
  l.iter().fold(
    "".to_string(),
    |a,b| if !a.is_empty() {a+sep} else {a} + &b.to_string()
  )
}

pub fn execute(benchmark_path: &str, report_path_option: Option<&str>, relaxed_interpolations: bool, no_check_certificate: bool, quiet: bool, nanosec: bool) -> BenchmarkResult {
  let config = Arc::new(Config::new(benchmark_path, relaxed_interpolations, no_check_certificate, quiet, nanosec));

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
  let mut rt = runtime::Builder::new().threaded_scheduler().enable_all().core_threads(threads).max_threads(threads).build().unwrap();
  rt.block_on(async {
    let mut benchmark: Benchmark = Benchmark::new();

    include::expand_from_filepath(benchmark_path, &mut benchmark, Some("plan"));

    let benchmark = Arc::new(benchmark);

    if let Some(report_path) = report_path_option {
      let reports = run_iteration(benchmark.clone(), config, 0).await;

      writer::write_file(report_path, join(reports, ""));

      BenchmarkResult {
        reports: vec![],
        duration: 0.0,
      }
    } else {
      let children = (0..config.iterations).map(|iteration| run_iteration(benchmark.clone(), config.clone(), iteration));

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
