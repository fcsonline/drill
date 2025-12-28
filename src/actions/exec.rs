use async_trait::async_trait;
use colored::*;
use serde_json::json;
use std::process::Command;
use serde_yaml::Value;

use crate::actions::Runnable;
use crate::actions::{extract, extract_optional};
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;
use crate::interpolator;

#[derive(Clone)]
pub struct Exec {
  name: String,
  command: String,
  pub assign: Option<String>,
}

impl Exec {
  pub fn is_that_you(item: &Value) -> bool {
    item.get("exec").and_then(|v| v.as_mapping()).is_some()
  }

  pub fn new(item: &Value, _with_item: Option<Value>) -> Exec {
    let name = extract(item, "name");
    let exec_val = item.get("exec").expect("exec field is required");
    let command = extract(exec_val, "command");
    let assign = extract_optional(item, "assign");

    Exec {
      name,
      command,
      assign,
    }
  }
}

#[async_trait]
impl Runnable for Exec {
  async fn execute(&self, context: &mut Context, _reports: &mut Reports, _pool: &Pool, config: &Config) {
    if !config.quiet {
      println!("{:width$} {}", self.name.green(), self.command.cyan().bold(), width = 25);
    }

    let final_command = interpolator::Interpolator::new(context).resolve(&self.command, !config.relaxed_interpolations);

    let args = ["bash", "-c", "--", final_command.as_str()];

    let execution = Command::new(args[0]).args(&args[1..]).output().expect("Couldn't run it");

    let output: String = String::from_utf8_lossy(&execution.stdout).into();
    let output = output.trim_end().to_string();

    if let Some(ref key) = self.assign {
      context.insert(key.to_owned(), json!(output));
    }
  }
}
