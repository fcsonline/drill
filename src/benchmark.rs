use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use yaml_rust::Yaml;

use crate::actions::{Report, Runnable};
use crate::expandable::include;
use crate::iteration::Iteration;
use crate::{config, writer};

use futures::prelude::*;
use futures::stream;

use colored::*;

fn thread_func(benchmark: Arc<Vec<Box<(Runnable + Sync + Send)>>>, config: Arc<config::Config>, thread: i64) -> Vec<Report> {
  let delay = config.rampup / config.threads;
  thread::sleep(std::time::Duration::new((delay * thread) as u64, 0));

  let mut global_reports = Vec::new();

  if config.extreme {
    // Extreme mode
    let responses = Arc::new(Mutex::new(HashMap::new()));
    let context: Arc<Mutex<HashMap<String, Yaml>>> = Arc::new(Mutex::new(HashMap::new()));
    let reports = Arc::new(Mutex::new(Vec::new()));

    let collector = reports.clone();

    let items: Vec<_> = (0..config.iterations).flat_map(|_idx| benchmark.iter().map(|item| item.execute(&context, &responses, &reports, &config))).collect();

    let benchmark_stream = stream::iter_ok::<_, ()>(items).buffer_unordered(500);

    tokio_scoped::scope(|scope| {
      scope.spawn({ benchmark_stream.for_each(|_n| Ok(())) });
    });

    let collected = collector.lock().unwrap();
    global_reports.push(collected.clone());
  } else if config.parallel {
    // Parallel mode
    let items: Vec<_> = (0..config.iterations)
      .map(|idx| {
        let responses = Arc::new(Mutex::new(HashMap::new()));
        let context: Arc<Mutex<HashMap<String, Yaml>>> = Arc::new(Mutex::new(HashMap::new()));
        let reports = Arc::new(Mutex::new(Vec::new()));

        let mut initial = context.lock().unwrap();
        initial.insert("iteration".to_string(), Yaml::String((idx + 1).to_string()));
        initial.insert("thread".to_string(), Yaml::String(thread.to_string()));
        initial.insert("base".to_string(), Yaml::String(config.base.to_string()));
        drop(initial);

        let iteration = Iteration {
          number: idx,
          responses: responses,
          context: context,
          reports: reports,
        };

        iteration
      })
      .collect();

    let iterations = items.iter().map(|item| item.future(&benchmark, &config));
    let work = futures::future::join_all(iterations).map(|_a| ()).map_err(|_a| ());

    tokio_scoped::scope(|scope| {
      scope.spawn(work);
    });
  } else {
    // Normal mode
    for idx in 0..config.iterations {
      let responses = Arc::new(Mutex::new(HashMap::new()));
      let context: Arc<Mutex<HashMap<String, Yaml>>> = Arc::new(Mutex::new(HashMap::new()));
      let reports = Arc::new(Mutex::new(Vec::new()));

      let mut initial = context.lock().unwrap();
      initial.insert("iteration".to_string(), Yaml::String((idx + 1).to_string()));
      initial.insert("thread".to_string(), Yaml::String(thread.to_string()));
      initial.insert("base".to_string(), Yaml::String(config.base.to_string()));
      drop(initial);

      let collector = reports.clone();

      let iteration = Iteration {
        number: idx,
        responses: responses,
        context: context,
        reports: reports,
      };

      let work = iteration.future(&benchmark, &config);

      tokio_scoped::scope(|scope| {
        scope.spawn(work);
      });

      let collected = collector.lock().unwrap();
      global_reports.push(collected.clone());
    }
  }

  global_reports.concat()
}

fn join<S: ToString>(l: Vec<S>, sep: &str) -> String {
  l.iter().fold("".to_string(), |a,b|
    if !a.is_empty() {a+sep} else {a} + &b.to_string()
  )
}

pub fn execute(benchmark_path: &str, report_path_option: Option<&str>, no_check_certificate: bool, quiet: bool, nanosec: bool, parallel: bool, extreme: bool) -> Result<Vec<Vec<Report>>, Vec<Vec<Report>>> {
  let config = Arc::new(config::Config::new(benchmark_path, no_check_certificate, quiet, nanosec, parallel, extreme));

  if report_path_option.is_some() {
    println!("{}: {}. Ignoring {} and {} properties...", "Report mode".yellow(), "on".purple(), "threads".yellow(), "iterations".yellow());
  } else {
    println!("{} {}", "Threads".yellow(), config.threads.to_string().purple());
    println!("{} {}", "Iterations".yellow(), config.iterations.to_string().purple());
    println!("{} {}", "Rampup".yellow(), config.rampup.to_string().purple());
  }

  println!("{} {}", "Base URL".yellow(), config.base.purple());
  println!("");

  let mut list: Vec<Box<(Runnable + Sync + Send)>> = Vec::new();

  include::expand_from_filepath(benchmark_path, &mut list, Some("plan"));

  if config.extreme && list.iter().any(|item| item.has_interpolations()) {
    panic!("Extreme mode incompatible with interpolations!");
  }

  let list_arc = Arc::new(list);
  let mut children = vec![];
  let mut list_reports: Vec<Vec<Report>> = vec![];

  if let Some(report_path) = report_path_option {
    let reports = thread_func(list_arc.clone(), config, 0);

    writer::write_file(report_path, join(reports, ""));

    Ok(list_reports)
  } else {
    for index in 0..config.threads {
      let list_clone = list_arc.clone();
      let config_clone = config.clone();
      children.push(thread::spawn(move || thread_func(list_clone, config_clone, index)));
    }

    for child in children {
      // Wait for the thread to finish. Returns a result.
      let thread_result = child.join();

      match thread_result {
        Ok(v) => list_reports.push(v),
        Err(_) => panic!("arrrgh"),
      }
    }

    Ok(list_reports)
  }
}
