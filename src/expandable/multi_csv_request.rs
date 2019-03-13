use yaml_rust::Yaml;
use std::path::Path;

use actions::{Request, Runnable};
use reader;

pub fn is_that_you(item: &Yaml) -> bool{
  item["request"].as_hash().is_some() &&
    item["with_items_from_csv"].as_str().is_some()
}

pub fn expand(parent_path: &str, item: &Yaml, list: &mut Vec<Box<(Runnable + Sync + Send)>>) {
  let with_items_from_csv_option = item["with_items_from_csv"].as_str();

  if with_items_from_csv_option.is_some() {
    let with_items_path = with_items_from_csv_option.unwrap();
    let with_items_filepath = Path::new(parent_path).with_file_name(with_items_path);
    let final_path = with_items_filepath.to_str().unwrap();

    let with_items_file = reader::read_csv_file_as_yml(final_path);

    for with_item in with_items_file {
      list.push(Box::new(Request::new(item, Some(with_item))));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use actions::Runnable;

  #[test]
  fn expand_multi () {
    let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item.id }}\nwith_items_from_csv: example/fixtures/users.csv";
    let docs = yaml_rust::YamlLoader::load_from_str(text).unwrap();
    let doc = &docs[0];
    let mut list: Vec<Box<(Runnable + Sync + Send)>> = Vec::new();

    expand("./", &doc, &mut list);

    assert_eq!(is_that_you(&doc), true);
    assert_eq!(list.len(), 2);
  }
}
