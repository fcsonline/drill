use std::process;
use yaml_rust::{YamlLoader, Yaml};

use expandable::{multi_request, multi_csv_request, include};
use actions;
use actions::Runnable;

use reader;

pub fn is_that_you(item: &Yaml) -> bool{
  item["include"].as_str().is_some()
}

pub fn expand(item: &Yaml, mut list: &mut Vec<Box<(Runnable + Sync + Send)>>) {
  let path = item["include"].as_str().unwrap();

  expand_from_filepath(path, &mut list, None);
}

pub fn expand_from_filepath(path: &str, mut list: &mut Vec<Box<(Runnable + Sync + Send)>>, accessor: Option<&str>) {
  let benchmark_file = reader::read_file(path);

  let docs = YamlLoader::load_from_str(benchmark_file.as_str()).unwrap();
  let doc = &docs[0];
  let items;

  if let Some(accessor_id) = accessor {
    items = match doc[accessor_id].as_vec() {
        Some(items) => items,
        None => {
            println!("{} {}", "Node missing on config:", accessor_id);
            println!("{}", "Exiting.");
            process::exit(1)
        },
    }
  } else {
    items = doc.as_vec().unwrap();
  }

  for item in items {
    if multi_request::is_that_you(item) {
      multi_request::expand(item, &mut list);
    } else if multi_csv_request::is_that_you(item) {
      multi_csv_request::expand(item, &mut list);
    } else if include::is_that_you(item) {
      include::expand(item, &mut list);
    } else if actions::Assign::is_that_you(item) {
      list.push(Box::new(actions::Assign::new(item, None)));
    } else if actions::Request::is_that_you(item){
      list.push(Box::new(actions::Request::new(item, None)));
    }
  }
}
