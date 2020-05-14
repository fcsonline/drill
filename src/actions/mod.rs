use async_trait::async_trait;

mod assign;
mod request;

pub use self::assign::Assign;
pub use self::request::Request;
use crate::config;

use reqwest::Client;
use std::collections::HashMap;
use std::fmt;

use serde_json::Value;
use yaml_rust::Yaml;

#[async_trait]
pub trait Runnable {
  async fn execute(&self, context: &mut HashMap<String, Yaml>, responses: &mut HashMap<String, Value>, reports: &mut Vec<Report>, pool: &mut HashMap<String, Client>, config: &config::Config);
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
