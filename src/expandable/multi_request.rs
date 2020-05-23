use yaml_rust::Yaml;

use crate::benchmark::Benchmark;
use crate::actions::Request;

pub fn is_that_you(item: &Yaml) -> bool {
  item["request"].as_hash().is_some() && item["with_items"].as_vec().is_some()
}

pub fn expand(item: &Yaml, benchmark: &mut Benchmark) {
  if let Some(with_items) = item["with_items"].as_vec() {
    for with_item in with_items {
      benchmark.push(Box::new(Request::new(item, Some(with_item.clone()))));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn expand_multi() {
    let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\nwith_items:\n  - 1\n  - 2\n  - 3";
    let docs = yaml_rust::YamlLoader::load_from_str(text).unwrap();
    let doc = &docs[0];
    let mut benchmark: Benchmark = Benchmark::new();

    expand(&doc, &mut benchmark);

    assert_eq!(is_that_you(&doc), true);
    assert_eq!(benchmark.len(), 3);
  }
}
