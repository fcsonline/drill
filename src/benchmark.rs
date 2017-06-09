use std::thread;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use yaml_rust::Yaml;
use serde_json::Value;

use expandable::include;
use actions::Runnable;
use config;

use colored::*;

fn thread_func(benchmark_clone: Arc<Mutex<Vec<Box<(Runnable + Sync + Send)>>>>, iterations: i64, base_clone: String) {
  for _ in 0..iterations {
    let mut responses:HashMap<String, Value> = HashMap::new();
    let mut context:HashMap<String, Yaml> = HashMap::new();

    context.insert("base".to_string(), Yaml::String(base_clone.clone()));

    for item in benchmark_clone.lock().unwrap().iter() {
      item.execute(&mut context, &mut responses);
    }
  }
}

pub fn execute(path: &str) {
  let config = config::Config::new(path);
  let threads: i64 = config.threads;
  let iterations: i64 = config.iterations;
  let base: String = config.base;

  println!("{} {}", "Threads".yellow(), threads.to_string().purple());
  println!("{} {}", "Iterations".yellow(), iterations.to_string().purple());
  println!("{} {}", "Base URL".yellow(), base.to_string().purple());
  println!("");

  let mut list: Vec<Box<(Runnable + Sync + Send)>> = Vec::new();

  include::expand_from_filepath(path, &mut list, Some("plan"));

  let mut children = vec![];

  let foo = Arc::new(Mutex::new(list));

  for _ in 0..threads {
    let base_clone = base.to_owned();
    let benchmark_clone = foo.clone();

    children.push(thread::spawn(move || thread_func(benchmark_clone, iterations, base_clone)));
  }

  for child in children {
    // Wait for the thread to finish. Returns a result.
    let _ = child.join();
  }
}
