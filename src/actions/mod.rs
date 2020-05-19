use async_trait::async_trait;

mod assign;
mod delay;
mod request;

pub use self::assign::Assign;
pub use self::delay::Delay;
pub use self::request::Request;

use crate::benchmark::{Context, Pool, Reports, Responses};
use crate::config::Config;

use std::fmt;

#[async_trait]
pub trait Runnable {
  async fn execute(&self, context: &mut Context, responses: &mut Responses, reports: &mut Reports, pool: &mut Pool, config: &Config);
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
