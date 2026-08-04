use async_trait::async_trait;
use colored::*;
use rand::seq::SliceRandom;
use serde_json::{Value, json};
use serde_yaml::Value as YamlValue;

use crate::actions::{Runnable, extract, extract_optional};
use crate::benchmark::{Benchmark, Context, Pool, Reports};
use crate::config::Config;
use crate::expandable::include;
use crate::interpolator;
use crate::tags::Tags;

pub struct ForEach {
  name: String,
  items: String,
  item_key: String,
  index_key: Option<String>,
  shuffle: bool,
  pick: Option<usize>,
  weight: u32,
  plan: Benchmark,
}

impl ForEach {
  pub fn is_that_you(item: &YamlValue) -> bool {
    item.get("for_each").and_then(|v| v.as_mapping()).is_some()
  }

  pub fn new(item: &YamlValue, parent_path: &str, tags: &Tags) -> ForEach {
    let name = extract_optional(item, "name").unwrap_or_default();
    let for_each = item.get("for_each").expect("for_each field is required");

    let items = extract(for_each, "items");
    let item_key = extract_optional(for_each, "item_key").unwrap_or_else(|| "item".to_string());
    let index_key = extract_optional(for_each, "index_key");
    let shuffle = for_each.get("shuffle").and_then(|v| v.as_bool()).unwrap_or(false);
    let pick = for_each.get("pick").and_then(|v| v.as_i64()).map(|v| v as usize);
    let weight = item.get("weight").and_then(|v| v.as_u64()).map(|v| v as u32).unwrap_or(1);

    let mut plan = Benchmark::new();
    if let Some(plan_items) = for_each.get("plan").and_then(|v| v.as_sequence()) {
      include::expand_sequence(parent_path, plan_items, &mut plan, tags);
    }

    ForEach {
      name,
      items,
      item_key,
      index_key,
      shuffle,
      pick,
      weight,
      plan,
    }
  }
}

#[async_trait]
impl Runnable for ForEach {
  fn weight(&self) -> u32 {
    self.weight
  }

  async fn execute(&self, context: &mut Context, reports: &mut Reports, pool: &Pool, config: &Config) {
    if !config.quiet {
      println!("{:width$} {}", self.name.green(), self.items.cyan().bold(), width = 25);
    }

    let resolved = interpolator::Interpolator::new(context).resolve(&self.items, !config.relaxed_interpolations);

    if resolved.is_empty() {
      return;
    }

    let value = serde_json::from_str::<Value>(&resolved).unwrap_or(Value::Null);

    let mut items = match value {
      Value::Array(arr) => arr,
      _ => {
        eprintln!("{} for_each items must resolve to a JSON array, got: {}", "WARNING!".yellow().bold(), resolved);
        return;
      }
    };

    if self.shuffle {
      let mut rng = rand::rng();
      items.shuffle(&mut rng);
    }

    if let Some(pick) = self.pick {
      items.truncate(pick);
    }

    for (index, item) in items.iter().enumerate() {
      let mut child_context = context.clone();
      child_context.insert(self.item_key.clone(), item.clone());
      if let Some(ref key) = self.index_key {
        child_context.insert(key.clone(), json!(index.to_string()));
      }

      for runnable in self.plan.iter() {
        runnable.execute(&mut child_context, reports, pool, config).await;
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;
  use std::collections::HashMap;

  fn empty_config() -> Config {
    Config {
      base: String::new(),
      concurrency: 1,
      iterations: 1,
      relaxed_interpolations: false,
      no_check_certificate: false,
      rampup: 0,
      quiet: true,
      nanosec: false,
      timeout: 10,
      verbose: false,
      results: None,
      lifecycle: Default::default(),
      load_shape: None,
      vars: HashMap::new(),
    }
  }

  fn test_context() -> Context {
    let mut context = Context::new();
    context.insert("users".to_string(), json!({
      "body": [
        { "id": 1, "name": "Alice" },
        { "id": 2, "name": "Bob" },
        { "id": 3, "name": "Carol" }
      ]
    }));
    context
  }

  #[test]
  fn is_that_you_detects_for_each() {
    let text = "---\nname: Iterate\nfor_each:\n  items: '{{ users.body }}'\n  plan: []";
    let docs = crate::reader::read_file_as_yml_from_str(text);
    let doc = &docs[0];

    assert!(ForEach::is_that_you(doc));
  }

  #[test]
  fn expand_for_each_plan() {
    let text = "---\nname: Iterate\nfor_each:\n  items: '{{ users.body }}'\n  item_key: user\n  plan:\n    - name: Fetch user\n      request:\n        url: /api/users/{{ user.id }}";
    let docs = crate::reader::read_file_as_yml_from_str(text);
    let doc = &docs[0];
    let for_each = ForEach::new(doc, "example/benchmark.yml", &Tags::new(None, None));

    assert_eq!(for_each.plan.len(), 1);
    assert_eq!(for_each.item_key, "user");
  }

  #[tokio::test]
  async fn executes_sub_plan_for_each_item() {
    let text = "---\nname: Iterate\nfor_each:\n  items: '{{ users.body }}'\n  item_key: user\n  plan:\n    - name: Count\n      assign:\n        key: count\n        value: '1'";
    let docs = crate::reader::read_file_as_yml_from_str(text);
    let doc = &docs[0];
    let for_each = ForEach::new(doc, "example/benchmark.yml", &Tags::new(None, None));

    let mut context = test_context();
    let mut reports = Vec::new();
    let pool = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));

    for_each.execute(&mut context, &mut reports, &pool, &empty_config()).await;
  }

  #[tokio::test]
  async fn pick_limits_executed_items() {
    let text = "---\nname: Iterate\nfor_each:\n  items: '{{ users.body }}'\n  pick: 2\n  plan:\n    - name: Count\n      assign:\n        key: count\n        value: '1'";
    let docs = crate::reader::read_file_as_yml_from_str(text);
    let doc = &docs[0];
    let for_each = ForEach::new(doc, "example/benchmark.yml", &Tags::new(None, None));

    let mut context = test_context();
    let mut reports = Vec::new();
    let pool = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));

    for_each.execute(&mut context, &mut reports, &pool, &empty_config()).await;
  }

  #[tokio::test]
  async fn missing_items_is_no_op_with_relaxed_interpolations() {
    let text = "---\nname: Iterate\nfor_each:\n  items: '{{ missing }}'\n  plan:\n    - name: Count\n      assign:\n        key: count\n        value: '1'";
    let docs = crate::reader::read_file_as_yml_from_str(text);
    let doc = &docs[0];
    let for_each = ForEach::new(doc, "example/benchmark.yml", &Tags::new(None, None));

    let mut context = Context::new();
    let mut reports = Vec::new();
    let pool = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut config = empty_config();
    config.relaxed_interpolations = true;

    for_each.execute(&mut context, &mut reports, &pool, &config).await;
  }
}
