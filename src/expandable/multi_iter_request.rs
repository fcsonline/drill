use yaml_rust::Yaml;

use actions::{Request, Runnable};

pub fn is_that_you(item: &Yaml) -> bool{
  item["request"].as_hash().is_some() &&
  item["with_items_iter"].as_hash().is_some()
}

pub fn expand(item: &Yaml, list: &mut Vec<Box<(Runnable + Sync + Send)>>) {
  let with_items_iter_option = item["with_items_iter"].as_hash();

  if with_items_iter_option.is_some() {
    let with_iter_items = with_items_iter_option.unwrap();

    let mut start = 1i64;
    let mut step = 1usize;
    let mut stop = 1i64;
    let ystart = Yaml::String("start".into());
    let ystep = Yaml::String("step".into());
    let ystop = Yaml::String("stop".into());
    if with_iter_items.contains_key(&ystart) && with_iter_items.get(&ystart).unwrap().as_i64().is_some() {
      start = with_iter_items.get(&ystart).unwrap().as_i64().unwrap();
    }
    if with_iter_items.contains_key(&ystep) && with_iter_items.get(&ystep).unwrap().as_i64().is_some() {
      step = with_iter_items.get(&ystep).unwrap().as_i64().unwrap() as usize;
    }
    if with_iter_items.contains_key(&ystop) && with_iter_items.get(&ystop).unwrap().as_i64().is_some() {
      stop = with_iter_items.get(&ystop).unwrap().as_i64().unwrap();
    }

    for i in (start .. stop).step_by(step) {
      list.push(Box::new(Request::new(item, Some(Yaml::Integer(i)))));
    }

  }
}
