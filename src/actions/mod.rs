mod assign;
mod request;
mod ws_message;

pub use self::assign::Assign;
pub use self::request::Request;
pub use self::ws_message::WSMessage;
use config;

use std::fmt;
use std::collections::HashMap;

use yaml_rust::Yaml;
use serde_json::Value;

pub trait Runnable {
  fn execute(&self, context: &mut HashMap<String, Yaml>, responses: &mut HashMap<String, Value>, reports: &mut Vec<Report>, config: &config::Config);
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
