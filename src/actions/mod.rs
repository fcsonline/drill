mod assign;
mod delay;
mod request;

pub use self::assign::Assign;
pub use self::delay::Delay;
pub use self::request::Request;

use futures::Future;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use yaml_rust::Yaml;

use crate::config;

pub trait Runnable {
  fn execute<'a>(
    &'a self,
    context: &'a Arc<Mutex<HashMap<String, Yaml>>>,
    responses: &'a Arc<Mutex<HashMap<String, serde_json::Value>>>,
    reports: &'a Arc<Mutex<Vec<Report>>>,
    config: &'a config::Config,
  ) -> (Box<Future<Item = (), Error = ()> + Send + 'a>);
  fn has_interpolations(&self) -> bool;
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
