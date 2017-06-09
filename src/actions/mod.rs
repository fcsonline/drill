mod assign;
mod request;

pub use self::assign::Assign;
pub use self::request::Request;

use std::fmt;
use std::collections::HashMap;

use yaml_rust::Yaml;
use serde_json::Value;

pub trait Runnable {
  fn execute(&self, context: &mut HashMap<String, Yaml>, responses: &mut HashMap<String, Value>, reports: &mut Vec<Report>);
}

pub struct Report {
  pub name: String,
  pub duration: f64,
}

impl fmt::Debug for Report {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "\n- name: {}\n  duration: {}\n", self.name, self.duration)
  }
}

impl fmt::Display for Report {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "\n- name: {}\n  duration: {}\n", self.name, self.duration)
  }
}
