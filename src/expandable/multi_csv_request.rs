use yaml_rust::Yaml;
use std::path::Path;

use actions::{Request, Runnable};
use reader;

pub fn is_that_you(item: &Yaml) -> bool{
  item["request"].as_hash().is_some() &&
    (item["with_items_from_csv"].as_str().is_some() ||
    item["with_items_from_csv"].as_hash().is_some())
}

pub fn expand(parent_path: &str, item: &Yaml, list: &mut Vec<Box<(Runnable + Sync + Send)>>) {
  let with_items_from_csv_option = item["with_items_from_csv"].as_str();
  let mut quote_char : u8 = '\"' as u8;
  let with_items_path : &str;

  if with_items_from_csv_option.is_some() {
    // handle the single string case
    with_items_path = with_items_from_csv_option.unwrap();
  }
  else if item["with_items_from_csv"].as_hash().is_some() {
    // handle the hash (file_name and quote_char) case
    with_items_path = item["with_items_from_csv"]["file_name"].as_str().unwrap();

    if item["with_items_from_csv"]["quote_char"].as_str().is_some() {
      quote_char = item["with_items_from_csv"]["quote_char"].as_str().unwrap().bytes().nth(0).unwrap();
    }

  }
  else { 
    return
  }
  let with_items_filepath = Path::new(parent_path).with_file_name(with_items_path);
  let final_path = with_items_filepath.to_str().unwrap();

  let with_items_file = reader::read_csv_file_as_yml(final_path, quote_char);

  for with_item in with_items_file {
    list.push(Box::new(Request::new(item, Some(with_item))));
  }
}
