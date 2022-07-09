use crate::benchmark::Benchmark;
use crate::interpolator::INTERPOLATION_REGEX;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::path::Path;
use yaml_rust::Yaml;

use crate::actions::Request;
use crate::reader;

pub fn is_that_you(item: &Yaml) -> bool {
  item["request"].as_hash().is_some() && (item["with_items_from_file"].as_str().is_some() || item["with_items_from_file"].as_hash().is_some())
}

pub fn expand(parent_path: &str, item: &Yaml, benchmark: &mut Benchmark) {
  let with_items_path = if let Some(with_items_path) = item["with_items_from_file"].as_str() {
    with_items_path
  } else {
    unreachable!();
  };

  if INTERPOLATION_REGEX.is_match(with_items_path) {
    panic!("Interpolation not supported in 'with_items_from_file' property!");
  }

  let with_items_filepath = Path::new(parent_path).with_file_name(with_items_path);
  let final_path = with_items_filepath.to_str().unwrap();

  let mut with_items_file = reader::read_file_as_yml_array(final_path);

  if let Some(shuffle) = item["shuffle"].as_bool() {
    if shuffle {
      let mut rng = thread_rng();
      with_items_file.shuffle(&mut rng);
    }
  }

  for (index, with_item) in with_items_file.iter().enumerate() {
    let index = index as u32;

    benchmark.push(Box::new(Request::new(item, Some(with_item.clone()), Some(index))));
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn expand_multi() {
    let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item.id }}\nwith_items_from_file: ./fixtures/texts.txt";
    let docs = yaml_rust::YamlLoader::load_from_str(text).unwrap();
    let doc = &docs[0];
    let mut benchmark: Benchmark = Benchmark::new();

    expand("example/benchmark.yml", doc, &mut benchmark);

    assert_eq!(is_that_you(doc), true);
    assert_eq!(benchmark.len(), 3);
  }
}
