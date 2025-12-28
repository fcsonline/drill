use async_trait::async_trait;
use colored::*;
use serde_json::json;
use serde_yaml::Value;

use crate::actions::extract;
use crate::actions::Runnable;
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;

#[derive(Clone)]
pub struct Assign {
  name: String,
  key: String,
  value: String,
}

impl Assign {
  pub fn is_that_you(item: &Value) -> bool {
    item.get("assign").and_then(|v| v.as_mapping()).is_some()
  }

  pub fn new(item: &Value, _with_item: Option<Value>) -> Assign {
    let name = extract(item, "name");
    let assign_val = item.get("assign").expect("assign field is required");
    let key = extract(assign_val, "key");
    let value = extract(assign_val, "value");

    Assign {
      name,
      key,
      value,
    }
  }
}

#[async_trait]
impl Runnable for Assign {
  async fn execute(&self, context: &mut Context, _reports: &mut Reports, _pool: &Pool, config: &Config) {
    if !config.quiet {
      println!("{:width$} {}={}", self.name.green(), self.key.cyan().bold(), self.value.magenta(), width = 25);
    }

    context.insert(self.key.to_owned(), json!(self.value.to_owned()));
  }
}
