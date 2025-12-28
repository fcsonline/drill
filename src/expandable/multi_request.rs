use rand::seq::SliceRandom;
use rand::thread_rng;
use serde_yaml::Value;

use super::pick;
use crate::actions::Request;
use crate::benchmark::Benchmark;
use crate::interpolator::INTERPOLATION_REGEX;

pub fn is_that_you(item: &Value) -> bool {
  item.get("request").and_then(|v| v.as_mapping()).is_some() && item.get("with_items").and_then(|v| v.as_sequence()).is_some()
}

pub fn expand(item: &Value, benchmark: &mut Benchmark) {
  if let Some(with_items) = item.get("with_items").and_then(|v| v.as_sequence()) {
    let mut with_items_list = with_items.clone();

    if let Some(shuffle) = item.get("shuffle").and_then(|v| v.as_bool()) {
      if shuffle {
        let mut rng = thread_rng();
        with_items_list.shuffle(&mut rng);
      }
    }

    let pick = pick(item, &with_items_list);
    for (index, with_item) in with_items_list.iter().take(pick).enumerate() {
      let index = index as u32;

      let value: &str = with_item.as_str().unwrap_or("");

      if INTERPOLATION_REGEX.is_match(value) {
        panic!("Interpolations not supported in 'with_items' children!");
      }

      benchmark.push(Box::new(Request::new(item, Some(with_item.clone()), Some(index))));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn expand_multi() {
    let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\nwith_items:\n  - 1\n  - 2\n  - 3";
    let docs = crate::reader::read_file_as_yml_from_str(text);
    let doc = &docs[0];
    let mut benchmark: Benchmark = Benchmark::new();

    expand(doc, &mut benchmark);

    assert!(is_that_you(doc));
    assert_eq!(benchmark.len(), 3);
  }

  #[test]
  fn expand_multi_should_limit_requests_using_the_pick_option() {
    let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\npick: 2\nwith_items:\n  - 1\n  - 2\n  - 3";
    let docs = crate::reader::read_file_as_yml_from_str(text);
    let doc = &docs[0];
    let mut benchmark: Benchmark = Benchmark::new();

    expand(doc, &mut benchmark);

    assert!(is_that_you(doc));
    assert_eq!(benchmark.len(), 2);
  }

  #[test]
  fn expand_multi_should_work_with_pick_and_shuffle() {
    let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\npick: 1\nshuffle: true\nwith_items:\n  - 1\n  - 2\n  - 3";
    let docs = crate::reader::read_file_as_yml_from_str(text);
    let doc = &docs[0];
    let mut benchmark: Benchmark = Benchmark::new();

    expand(doc, &mut benchmark);

    assert!(is_that_you(doc));
    assert_eq!(benchmark.len(), 1);
  }

  #[test]
  #[should_panic]
  fn runtime_expand() {
    let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\nwith_items:\n  - 1\n  - 2\n  - foo{{ memory }}";
    let docs = crate::reader::read_file_as_yml_from_str(text);
    let doc = &docs[0];
    let mut benchmark: Benchmark = Benchmark::new();

    expand(doc, &mut benchmark);
  }
}
