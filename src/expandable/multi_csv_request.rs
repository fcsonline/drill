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
    let (with_items_path, quote_char) =
        if let Some(with_items_path) = item["with_items_from_csv"].as_str() {
            (with_items_path, '\"' as u8)
        } else if let Some(_with_items_hash) = item["with_items_from_csv"].as_hash() {
            let with_items_path = item["with_items_from_csv"]["file_name"].as_str().expect("Expected a file_name");
            let quote_char = item["with_items_from_csv"]["quote_char"].as_str().unwrap_or("\"").bytes().nth(0).unwrap();

            (with_items_path, quote_char)
        } else {
            panic!("WAT"); // Impossible case
        };

    let with_items_filepath = Path::new(parent_path).with_file_name(with_items_path);
    let final_path = with_items_filepath.to_str().unwrap();

    let with_items_file = reader::read_csv_file_as_yml(final_path, quote_char);

    for with_item in with_items_file {
        list.push(Box::new(Request::new(item, Some(with_item))));
    }
}
