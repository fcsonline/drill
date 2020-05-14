use std::collections::HashMap;

use async_trait::async_trait;
use colored::*;
use reqwest::Client;
use serde_json::Value;
use yaml_rust::Yaml;

use crate::config;

use crate::actions::{Report, Runnable};

#[derive(Clone)]
pub struct Assign {
  name: String,
  key: String,
  value: String,
}

impl Assign {
  pub fn is_that_you(item: &Yaml) -> bool {
    item["assign"].as_hash().is_some()
  }

  pub fn new(item: &Yaml, _with_item: Option<Yaml>) -> Assign {
    Assign {
      name: item["name"].as_str().unwrap().to_string(),
      key: item["assign"]["key"].as_str().unwrap().to_string(),
      value: item["assign"]["value"].as_str().unwrap().to_string(),
    }
  }
}

#[async_trait]
impl Runnable for Assign {
  async fn execute(&self, context: &mut HashMap<String, Yaml>, _responses: &mut HashMap<String, Value>, _reports: &mut Vec<Report>, _pool: &mut HashMap<String, Client>, _config: &config::Config) {
    if !_config.quiet {
      println!("{:width$} {}={}", self.name.green(), self.key.cyan().bold(), self.value.magenta(), width = 25);
    }

    context.insert(self.key.to_owned(), Yaml::String(self.value.to_owned()));
  }
}
