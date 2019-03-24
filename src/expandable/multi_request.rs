use yaml_rust::Yaml;

use actions::{Request, Runnable};

pub fn is_that_you(item: &Yaml) -> bool {
  item["request"].as_hash().is_some() && item["with_items"].as_vec().is_some()
}

pub fn expand(item: &Yaml, list: &mut Vec<Box<(Runnable + Sync + Send)>>) {
  if let Some(with_items) = item["with_items"].as_vec() {
    for with_item in with_items {
      list.push(Box::new(Request::new(item, Some(with_item.clone()))));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use actions::Runnable;

  #[test]
  fn expand_multi() {
    let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\nwith_items:\n  - 1\n  - 2\n  - 3";
    let docs = yaml_rust::YamlLoader::load_from_str(text).unwrap();
    let doc = &docs[0];
    let mut list: Vec<Box<(Runnable + Sync + Send)>> = Vec::new();

    expand(&doc, &mut list);

    assert_eq!(is_that_you(&doc), true);
    assert_eq!(list.len(), 3);
  }
}
