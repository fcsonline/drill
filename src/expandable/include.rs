use std::path::Path;
use std::process;
use yaml_rust::{Yaml, YamlEmitter, YamlLoader};

use crate::actions;
use crate::benchmark::Benchmark;
use crate::expandable::{include, multi_csv_request, multi_iter_request, multi_request};

use crate::reader;

pub fn is_that_you(item: &Yaml) -> bool {
  item["include"].as_str().is_some()
}

pub fn expand(parent_path: &str, item: &Yaml, mut benchmark: &mut Benchmark) {
  let include_path = item["include"].as_str().unwrap();
  let include_filepath = Path::new(parent_path).with_file_name(include_path);
  let final_path = include_filepath.to_str().unwrap();

  expand_from_filepath(final_path, &mut benchmark, None);
}

pub fn expand_from_filepath(parent_path: &str, mut benchmark: &mut Benchmark, accessor: Option<&str>) {
  let benchmark_file = reader::read_file(parent_path);

  let docs = YamlLoader::load_from_str(benchmark_file.as_str()).unwrap();
  let doc = &docs[0];
  let items;

  if let Some(accessor_id) = accessor {
    items = match doc[accessor_id].as_vec() {
      Some(items) => items,
      None => {
        println!("Node missing on config: {}", accessor_id);
        println!("Exiting.");
        process::exit(1)
      }
    }
  } else {
    items = doc.as_vec().unwrap();
  }

  for item in items {
    if multi_request::is_that_you(item) {
      multi_request::expand(item, &mut benchmark);
    } else if multi_iter_request::is_that_you(item) {
      multi_iter_request::expand(item, &mut benchmark);
    } else if multi_csv_request::is_that_you(item) {
      multi_csv_request::expand(parent_path, item, &mut benchmark);
    } else if include::is_that_you(item) {
      include::expand(parent_path, item, &mut benchmark);
    } else if actions::Delay::is_that_you(item) {
      benchmark.push(Box::new(actions::Delay::new(item, None)));
    } else if actions::Assign::is_that_you(item) {
      benchmark.push(Box::new(actions::Assign::new(item, None)));
    } else if actions::Request::is_that_you(item) {
      benchmark.push(Box::new(actions::Request::new(item, None)));
    } else {
      let mut out_str = String::new();
      let mut emitter = YamlEmitter::new(&mut out_str);
      emitter.dump(item).unwrap();
      panic!("Unknown node:\n\n{}\n\n", out_str);
    }
  }
}
