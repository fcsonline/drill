use colored::*;
use futures::Future;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use yaml_rust::Yaml;

use crate::actions::{Report, Runnable};
use crate::config;
use crate::interpolator::Interpolator;

use std::time::Duration;

#[derive(Clone)]
pub struct Delay {
  name: String,
  seconds: i64,
}

impl Delay {
  pub fn is_that_you(item: &Yaml) -> bool {
    item["delay"].as_hash().is_some()
  }

  pub fn new(item: &Yaml, _with_item: Option<Yaml>) -> Delay {
    Delay {
      name: item["name"].as_str().unwrap().to_string(),
      seconds: item["delay"]["seconds"].as_i64().unwrap(),
    }
  }
}

impl fmt::Debug for Delay {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "Delay: {}\n", self.name)
  }
}

impl Runnable for Delay {
  fn execute<'a>(
    &'a self,
    _context: &'a Arc<Mutex<HashMap<String, Yaml>>>,
    _responses: &'a Arc<Mutex<HashMap<String, serde_json::Value>>>,
    _reports: &'a Arc<Mutex<Vec<Report>>>,
    config: &'a config::Config,
  ) -> (Box<Future<Item = (), Error = ()> + Send + 'a>) {
    let work = futures_timer::Delay::new(Duration::from_secs(3))
      .map(move |()| {
        if !config.quiet {
          println!("{:width$} {}{}", self.name.green(), self.seconds.to_string().cyan().bold(), "s".magenta(), width = 25);
        }
      })
      .map_err(|err| println!("Timer error: {}", err));

    Box::new(work)
  }

  fn has_interpolations(&self) -> bool {
    Interpolator::has_interpolations(&self.name)
  }
}
