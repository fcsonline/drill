use async_trait::async_trait;
use serde_json::{json, Map, Value};
use yaml_rust::Yaml;

mod assert;
mod assign;
mod delay;
mod exec;
mod request;

pub use self::assert::Assert;
pub use self::assign::Assign;
pub use self::delay::Delay;
pub use self::exec::Exec;
pub use self::request::Request;

use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;

use std::fmt;

#[async_trait]
pub trait Runnable {
  async fn execute(&self, context: &mut Context, reports: &mut Reports, pool: &Pool, config: &Config);
}

#[derive(Clone)]
pub struct Report {
  pub name: String,
  pub duration: f64,
  pub status: u16,
}

impl fmt::Debug for Report {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "\n- name: {}\n  duration: {}\n", self.name, self.duration)
  }
}

impl fmt::Display for Report {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "\n- name: {}\n  duration: {}\n  status: {}\n", self.name, self.duration, self.status)
  }
}

pub fn extract_optional<'a>(item: &'a Yaml, attr: &'a str) -> Option<String> {
  if let Some(s) = item[attr].as_str() {
    Some(s.to_string())
  } else if item[attr].as_hash().is_some() {
    panic!("`{}` needs to be a string. Try adding quotes", attr);
  } else {
    None
  }
}

pub fn extract<'a>(item: &'a Yaml, attr: &'a str) -> String {
  if let Some(s) = item[attr].as_i64() {
    s.to_string()
  } else if let Some(s) = item[attr].as_str() {
    s.to_string()
  } else if item[attr].as_hash().is_some() {
    panic!("`{}` is required needs to be a string. Try adding quotes", attr);
  } else {
    panic!("Unknown node `{}` => {:?}", attr, item[attr]);
  }
}

pub fn yaml_to_json(data: Yaml) -> Value {
  if let Some(b) = data.as_bool() {
    json!(b)
  } else if let Some(i) = data.as_i64() {
    json!(i)
  } else if let Some(s) = data.as_str() {
    json!(s)
  } else if let Some(h) = data.as_hash() {
    let mut map = Map::new();

    for (key, value) in h.iter() {
      map.entry(key.as_str().unwrap()).or_insert(yaml_to_json(value.clone()));
    }

    json!(map)
  } else if let Some(v) = data.as_vec() {
    let mut array = Vec::new();

    for value in v.iter() {
      array.push(yaml_to_json(value.clone()));
    }

    json!(array)
  } else {
    panic!("Unknown Yaml node")
  }
}
