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

    let mut step = 1usize;
    let mut stop = 1i64;
    let init = Yaml::Integer(1);
    let ystart = Yaml::String("start".into());
    let ystep = Yaml::String("step".into());
    let ystop = Yaml::String("stop".into());
    
    let start : i64 = with_iter_items.get(&ystart).unwrap_or(&init).as_i64().unwrap_or(1);
    let step : usize = with_iter_items.get(&ystep).unwrap_or(&init).as_i64().unwrap_or(1) as usize;
    let stop : i64 = with_iter_items.get(&ystop).unwrap_or(&init).as_i64().unwrap_or(1) + 1; // making stop inclusive

    if stop > start {
      for i in (start .. stop).step_by(step) {
        list.push(Box::new(Request::new(item, Some(Yaml::Integer(i)))));
      }
    }

  }
}
