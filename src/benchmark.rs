use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};

use serde_json::{json, Map, Value};
use tokio::{runtime, time::sleep};

use crate::actions::{Report, Runnable};
use crate::cli::Args;
use crate::expandable::{self};
use crate::parser::BenchmarkConfig;
use crate::tags::Tags;
use crate::writer;

use reqwest::Client;

use colored::*;

pub type Benchmark = Vec<Box<dyn Runnable + Sync + Send>>;
pub type Context = Map<String, Value>;
pub type Reports = Vec<Report>;
pub type PoolStore = HashMap<String, Client>;
pub type Pool = Arc<Mutex<PoolStore>>;

pub struct BenchmarkResult {
    pub reports: Vec<Reports>,
    pub duration: f64,
}

async fn run_iteration(benchmark: Arc<Benchmark>, pool: Pool, benchmark_config: Arc<BenchmarkConfig>, iteration: usize) -> Result<Vec<Report>, io::Error> {
    if benchmark_config.rampup > 0 {
        let delay = benchmark_config.rampup / benchmark_config.iterations;
        sleep(Duration::new((delay * iteration) as u64, 0)).await;
    }

    let mut context: Context = Context::new();
    let mut reports: Vec<Report> = Vec::new();

    context.insert("iteration".to_string(), json!(iteration.to_string()));
    context.insert("base".to_string(), json!(benchmark_config.base.to_string()));

    for item in benchmark.iter() {
        item.execute(&mut context, &mut reports, &pool, &benchmark_config).await?;
    }

    Ok(reports)
}

fn join<S: ToString>(l: Vec<S>, sep: &str) -> String {
    l.iter().fold(
    "".to_string(),
    |a,b| if !a.is_empty() {a+sep} else {a} + &b.to_string()
  )
}

#[allow(clippy::too_many_arguments)]
pub fn execute(benchmark_config: Arc<BenchmarkConfig>, app_args: &Args, tags: &Tags) -> Result<BenchmarkResult, io::Error> {
    if app_args.report.is_some() {
        println!("{}: {}. Ignoring {} and {} properties...", "Report mode".yellow(), "on".purple(), "concurrency".yellow(), "iterations".yellow());
    } else {
        println!("{} {}", "Concurrency".yellow(), benchmark_config.concurrency.to_string().purple());
        println!("{} {}", "Iterations".yellow(), benchmark_config.iterations.to_string().purple());
        println!("{} {}", "Rampup".yellow(), benchmark_config.rampup.to_string().purple());
    }

    println!("{} {}", "Base URL".yellow(), benchmark_config.base.purple());
    println!();

    let threads = std::cmp::min(num_cpus::get(), benchmark_config.concurrency as usize);
    let rt = runtime::Builder::new_multi_thread().enable_all().worker_threads(threads).build()?;

    rt.block_on(async {
        let mut benchmark: Benchmark = Benchmark::new();
        let pool_store: PoolStore = PoolStore::new();

        expandable::expand_from_filepath(benchmark_path, &mut benchmark, Some("plan"), tags)?;

        if benchmark.is_empty() {
            eprintln!("Empty benchmark. Exiting.");
            std::process::exit(1);
        }

        let benchmark = Arc::new(benchmark);
        let pool = Arc::new(Mutex::new(pool_store));

        if let Some(report_path) = app_args.report {
            let reports = run_iteration(benchmark.clone(), pool.clone(), benchmark_config, 0).await?;

            writer::write_file(args.report, join(reports, ""))?;

            Ok(BenchmarkResult {
                reports: vec![],
                duration: 0.0,
            })
        } else {
            let children = (0..benchmark_config.iterations).map(|iteration| run_iteration(benchmark.clone(), pool.clone(), benchmark_config.clone(), iteration));

            let buffered = stream::iter(children).buffer_unordered(benchmark_config.concurrency as usize);

            let begin = Instant::now();

            let reports = {
                let mut reports: Vec<Vec<Report>> = Vec::new();
                for report in buffered.collect::<Vec<_>>().await {
                    reports.push(report?);
                }
                reports
            };

            let duration = begin.elapsed().as_secs_f64();

            Ok(BenchmarkResult {
                reports,
                duration,
            })
        }
    })
}
