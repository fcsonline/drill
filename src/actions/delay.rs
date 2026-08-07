use async_trait::async_trait;
use colored::*;
use serde_yaml::Value;
use tokio::time::sleep;

use crate::actions::Runnable;
use crate::actions::extract;
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;

use std::convert::TryFrom;
use std::time::Duration;

#[derive(Clone)]
pub struct Delay {
  name: String,
  seconds: u64,
  weight: u32,
}

impl Delay {
  pub fn is_that_you(item: &Value) -> bool {
    item.get("delay").and_then(|v| v.as_mapping()).is_some()
  }

  pub fn new(item: &Value, _with_item: Option<Value>) -> Delay {
    let name = extract(item, "name");
    let delay_val = item.get("delay").expect("delay field is required");
    let seconds = u64::try_from(delay_val.get("seconds").and_then(|v| v.as_i64()).expect("Invalid number of seconds")).expect("Invalid number of seconds");
    let weight = item.get("weight").and_then(|v| v.as_u64()).map(|v| v as u32).unwrap_or(1);

    Delay {
      name,
      seconds,
      weight,
    }
  }
}

#[async_trait]
impl Runnable for Delay {
  fn weight(&self) -> u32 {
    self.weight
  }

  async fn execute(&self, _context: &mut Context, _reports: &mut Reports, _pool: &Pool, config: &Config) {
    sleep(Duration::from_secs(self.seconds)).await;

    if !config.quiet {
      crate::emit(config.stats_json, format_args!("{:width$} {}{}", self.name.green(), self.seconds.to_string().cyan().bold(), "s".magenta(), width = 25));
    }
  }
}
