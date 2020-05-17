use std::collections::HashMap;
use std::sync::Arc;
use std::time;

use tokio::{runtime, time::delay_for};
use yaml_rust::Yaml;

use crate::actions::{Report, Runnable};
use crate::config;
use crate::expandable::include;
use crate::writer;

use reqwest::Client;

use colored::*;

async fn run_iterations(benchmark: Arc<Vec<Box<(dyn Runnable + Sync + Send)>>>, config: Arc<config::Config>, thread: i64) -> Vec<Report> {
  let delay = config.rampup / config.threads;
  delay_for(time::Duration::new((delay * thread) as u64, 0)).await;

  let mut global_reports = Vec::new();

  let mut pool: HashMap<String, Client> = HashMap::new();

  for iteration in 0..config.iterations {
    let mut responses: HashMap<String, serde_json::Value> = HashMap::new();
    let mut context: HashMap<String, Yaml> = HashMap::new();
    let mut reports: Vec<Report> = Vec::new();

    context.insert("iteration".to_string(), Yaml::String(iteration.to_string()));
    context.insert("thread".to_string(), Yaml::String(thread.to_string()));
    context.insert("base".to_string(), Yaml::String(config.base.to_string()));

    for item in benchmark.iter() {
      item.execute(&mut context, &mut responses, &mut reports, &mut pool, &config).await;
    }

    global_reports.push(reports);
  }

  global_reports.concat()
}

fn join<S: ToString>(l: Vec<S>, sep: &str) -> String {
  l.iter().fold("".to_string(),
                  |a,b| if !a.is_empty() {a+sep} else {a} + &b.to_string()
                  )
}

pub fn execute(benchmark_path: &str, report_path_option: Option<&str>, relaxed_interpolations: bool, no_check_certificate: bool, quiet: bool, nanosec: bool) -> Result<Vec<Vec<Report>>, Vec<Vec<Report>>> {
  let config = Arc::new(config::Config::new(benchmark_path, relaxed_interpolations, no_check_certificate, quiet, nanosec));

  if report_path_option.is_some() {
    println!("{}: {}. Ignoring {} and {} properties...", "Report mode".yellow(), "on".purple(), "threads".yellow(), "iterations".yellow());
  } else {
    println!("{} {}", "Threads".yellow(), config.threads.to_string().purple());
    println!("{} {}", "Iterations".yellow(), config.iterations.to_string().purple());
    println!("{} {}", "Rampup".yellow(), config.rampup.to_string().purple());
  }

  println!("{} {}", "Base URL".yellow(), config.base.purple());
  println!();

  let threads = config.threads as usize;
  let mut rt = runtime::Builder::new().threaded_scheduler().enable_all().core_threads(threads).max_threads(threads).build().unwrap();
  rt.block_on(async {
    let mut list: Vec<Box<(dyn Runnable + Sync + Send)>> = Vec::new();

    include::expand_from_filepath(benchmark_path, &mut list, Some("plan"));

    let list_arc = Arc::new(list);
    let mut children = vec![];

    if let Some(report_path) = report_path_option {
      let reports = run_iterations(list_arc.clone(), config, 0).await;

      writer::write_file(report_path, join(reports, ""));

      Ok(Vec::new())
    } else {
      for index in 0..config.concurrency {
        let list_clone = list_arc.clone();
        let config_clone = config.clone();
        children.push(tokio::spawn(async move { run_iterations(list_clone, config_clone, index).await }));
      }
      let list_reports: Vec<Vec<Report>> = futures::future::join_all(children).await.into_iter().map(|x| x.unwrap()).collect();
      Ok(list_reports)
    }
  })
}
