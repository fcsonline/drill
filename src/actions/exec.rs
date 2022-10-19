use async_trait::async_trait;
use colored::*;
use serde_json::json;
use std::process::Command;
use yaml_rust::Yaml;

use crate::actions::Runnable;
use crate::actions::{extract, extract_optional, yaml_to_json};
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;
use crate::interpolator;

#[derive(Clone)]
pub struct Exec {
  name: String,
  command: String,
  pub with_item: Option<Yaml>,
  pub index: Option<u32>,
  pub assign: Option<String>,
}

impl Exec {
  pub fn is_that_you(item: &Yaml) -> bool {
    item["exec"].as_hash().is_some()
  }

  pub fn new(item: &Yaml, with_item: Option<Yaml>, index: Option<u32>) -> Exec {
    let name = extract(item, "name");
    let command = extract(&item["exec"], "command");
    let assign = extract_optional(item, "assign");

    Exec {
      name,
      command,
      with_item,
      index,
      assign,
    }
  }
}

#[async_trait]
impl Runnable for Exec {
  async fn execute(&self, context: &mut Context, _reports: &mut Reports, _pool: &Pool, config: &Config) {
    if self.with_item.is_some() {
      context.insert("item".to_string(), yaml_to_json(self.with_item.clone().unwrap()));
    }
    if !config.quiet {
      if self.with_item.is_some() {
        println!("{:width$} ({}) {}", self.name.green(), self.with_item.clone().unwrap().as_str().unwrap(), self.command.cyan().bold(), width = 25);
      } else {
        println!("{:width$} {}", self.name.green(), self.command.cyan().bold(), width = 25);
      }
    }

    let final_command = interpolator::Interpolator::new(context).resolve(&self.command, !config.relaxed_interpolations);

    let args = vec!["bash", "-c", "--", final_command.as_str()];

    let execution = Command::new(args[0]).args(&args[1..]).output().expect("Couldn't run it");

    let output: String = String::from_utf8_lossy(&execution.stdout).into();
    let output = output.trim_end().to_string();
    if !config.quiet {
      if self.with_item.is_some() {
        println!("{:width$} ({}) >>> {}", self.name.green(), self.with_item.clone().unwrap().as_str().unwrap(), output.cyan().bold(), width = 25);
      } else {
        println!("{:width$} >>> {}", self.name.green(), output.cyan().bold(), width = 25);
      }
    }

    if let Some(ref key) = self.assign {
      context.insert(key.to_owned(), json!(output));
    }
  }
}
