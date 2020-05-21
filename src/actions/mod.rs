use async_trait::async_trait;
use yaml_rust::Yaml;

mod assign;
mod delay;
mod request;

pub use self::assign::Assign;
pub use self::delay::Delay;
pub use self::request::Request;

use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;

use std::fmt;

#[async_trait]
pub trait Runnable {
  async fn execute(&self, context: &mut Context, reports: &mut Reports, pool: &mut Pool, config: &Config);
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

pub fn extract_optional<'a>(item: &'a Yaml, attr: &'a str) -> Option<&'a str> {
  if let Some(s) = item[attr].as_str() {
    Some(s)
  } else {
    if item[attr].as_hash().is_some() {
      panic!("`{}` needs to be a string. Try adding quotes", attr);
    } else {
      None
    }
  }
}

pub fn extract<'a>(item: &'a Yaml, attr: &'a str) -> &'a str {
  if let Some(s) = item[attr].as_str() {
    s
  } else {
    if item[attr].as_hash().is_some() {
      panic!("`{}` is required needs to be a string. Try adding quotes", attr);
    } else {
      panic!("Unknown node `{}` => {:?}", attr, item[attr]);
    }
  }
}
