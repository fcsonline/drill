use std::thread;
use std::collections::HashMap;

extern crate yaml_rust;
use self::yaml_rust::Yaml;

extern crate serde_json;
use self::serde_json::Value;

extern crate time;

use expandable::include;
use actions::Runnable;

use std::sync::{Arc, Mutex};

fn thread_func(benchmark_clone: Arc<Mutex<Vec<Box<(Runnable + Sync + Send)>>>>, iterations: i64, base_url_clone: String) {
  for _ in 0..iterations {
    let mut responses:HashMap<String, Value> = HashMap::new();
    let mut context:HashMap<String, Yaml> = HashMap::new();

    for item in benchmark_clone.lock().unwrap().iter() {
      item.execute(&base_url_clone, &mut context, &mut responses);
    }
  }
}

pub fn execute(path: &str, threads: i64, iterations: i64, base_url: String) {
  let mut list: Vec<Box<(Runnable + Sync + Send)>> = Vec::new();

  include::expand_from_filepath(path, &mut list);

  let mut children = vec![];

  let foo = Arc::new(Mutex::new(list));

  for _ in 0..threads {
    let base_url_clone = base_url.to_owned();
    let benchmark_clone = foo.clone();

    children.push(thread::spawn(move || thread_func(benchmark_clone, iterations, base_url_clone)));
  }

  for child in children {
    // Wait for the thread to finish. Returns a result.
    let _ = child.join();
  }
}
