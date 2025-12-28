use async_trait::async_trait;
use colored::*;
use serde_json::json;
use serde_yaml::Value;

use crate::actions::extract;
use crate::actions::Runnable;
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;
use crate::interpolator;

#[derive(Clone)]
pub struct Assert {
  name: String,
  key: String,
  value: String,
}

impl Assert {
  pub fn is_that_you(item: &Value) -> bool {
    item.get("assert").and_then(|v| v.as_mapping()).is_some()
  }

  pub fn new(item: &Value, _with_item: Option<Value>) -> Assert {
    let name = extract(item, "name");
    let assert_val = item.get("assert").expect("assert field is required");
    let key = extract(assert_val, "key");
    let value = extract(assert_val, "value");

    Assert {
      name,
      key,
      value,
    }
  }
}

#[async_trait]
impl Runnable for Assert {
  async fn execute(&self, context: &mut Context, _reports: &mut Reports, _pool: &Pool, config: &Config) {
    if !config.quiet {
      println!("{:width$} {}={}?", self.name.green(), self.key.cyan().bold(), self.value.magenta(), width = 25);
    }

    let interpolator = interpolator::Interpolator::new(context);
    let eval = format!("{{{{ {} }}}}", &self.key);
    let stored = interpolator.resolve(&eval, true);
    let assertion = json!(self.value.to_owned());

    if !stored.eq(&assertion) {
      panic!("Assertion mismatched: {stored} != {assertion}");
    }
  }
}
