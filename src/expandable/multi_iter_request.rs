use yaml_rust::Yaml;

use crate::benchmark::Benchmark;
use crate::actions::Request;

pub fn is_that_you(item: &Yaml) -> bool {
  item["request"].as_hash().is_some() && item["with_items_range"].as_hash().is_some()
}

pub fn expand(item: &Yaml, benchmark: &mut Benchmark) {
  if let Some(with_iter_items) = item["with_items_range"].as_hash() {
    let init = Yaml::Integer(1);
    let ystart = Yaml::String("start".into());
    let ystep = Yaml::String("step".into());
    let ystop = Yaml::String("stop".into());

    let start: i64 = with_iter_items.get(&ystart).unwrap_or(&init).as_i64().unwrap_or(1);
    let step: usize = with_iter_items.get(&ystep).unwrap_or(&init).as_i64().unwrap_or(1) as usize;
    let stop: i64 = with_iter_items.get(&ystop).unwrap_or(&init).as_i64().unwrap_or(1) + 1; // making stop inclusive

    if stop > start && start > 0 {
      for i in (start..stop).step_by(step) {
        benchmark.push(Box::new(Request::new(item, Some(Yaml::Integer(i)))));
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn expand_multi_range() {
    let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\nwith_items_range:\n  start: 2\n  step: 2\n  stop: 20";
    let docs = yaml_rust::YamlLoader::load_from_str(text).unwrap();
    let doc = &docs[0];
    let mut benchmark: Benchmark = Benchmark::new();

    expand(&doc, &mut benchmark);

    assert_eq!(is_that_you(&doc), true);
    assert_eq!(benchmark.len(), 10);
  }
}
