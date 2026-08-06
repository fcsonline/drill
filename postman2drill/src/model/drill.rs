use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Drill output structures

#[derive(Debug, Serialize, Default)]
pub struct DrillBenchmark {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub base: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub concurrency: Option<i64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub iterations: Option<i64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub rampup: Option<i64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub vars: Option<HashMap<String, serde_yaml::Value>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub lifecycle: Option<Lifecycle>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub results: Option<ResultsConfig>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub load_shape: Option<LoadShape>,
  pub plan: Vec<PlanItem>,
}

#[derive(Debug, Serialize, Default)]
pub struct Lifecycle {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub setup: Option<Vec<PlanItem>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub teardown: Option<Vec<PlanItem>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub iteration_start: Option<Vec<PlanItem>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub iteration_stop: Option<Vec<PlanItem>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ResultsConfig {
  pub output_dir: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub csv: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub html: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LoadShape {
  pub stages: Vec<LoadShapeStage>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LoadShapeStage {
  pub duration: u64,
  pub users: u64,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub spawn_rate: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DrillConfigInput {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub base: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub concurrency: Option<i64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub iterations: Option<i64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub rampup: Option<i64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub vars: Option<HashMap<String, serde_yaml::Value>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub results: Option<ResultsConfig>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub load_shape: Option<LoadShape>,
}

#[allow(clippy::large_enum_variant, dead_code)]
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum PlanItem {
  Request(RequestItem),
  Assign(AssignItem),
  Assert(AssertItem),
  Save(SaveItem),
  Exec(ExecItem),
  Delay(DelayItem),
  ForEach(ForEachItem),
  Include(String),
}

#[derive(Debug, Serialize)]
pub struct RequestItem {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  pub request: DrillRequest,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub assign: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub weight: Option<u32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub tags: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct DrillRequest {
  pub url: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub method: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub headers: Option<HashMap<String, String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub body: Option<DrillBody>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub auth: Option<DrillAuth>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub with_items: Option<Vec<serde_yaml::Value>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub with_items_range: Option<WithItemsRange>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub with_items_from_csv: Option<WithItemsFromCsv>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub shuffle: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub pick: Option<usize>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub enum DrillBody {
  Template(String),
  Binary {
    hex: String,
  },
  BinaryFile {
    file: String,
  },
  UrlEncoded(HashMap<String, String>),
  FormData(Vec<FormDataPart>),
  GraphQL {
    query: String,
    variables: Option<HashMap<String, String>>,
  },
}

impl Serialize for DrillBody {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    let value = match self {
      DrillBody::Template(s) => serde_yaml::Value::String(s.clone()),
      DrillBody::Binary {
        hex,
      } => serde_yaml::to_value(serde_json::json!({ "hex": hex })).unwrap(),
      DrillBody::BinaryFile {
        file,
      } => serde_yaml::to_value(serde_json::json!({ "file": file })).unwrap(),
      DrillBody::UrlEncoded(map) => serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter(vec![(serde_yaml::Value::String("urlencoded".to_string()), serde_yaml::to_value(map).unwrap())])),
      DrillBody::FormData(parts) => serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter(vec![(serde_yaml::Value::String("formdata".to_string()), serde_yaml::to_value(parts).unwrap())])),
      DrillBody::GraphQL {
        query,
        variables,
      } => {
        let mut graphql_map = serde_yaml::Mapping::new();
        graphql_map.insert(serde_yaml::Value::String("query".to_string()), serde_yaml::Value::String(query.clone()));
        if let Some(vars) = variables {
          graphql_map.insert(serde_yaml::Value::String("variables".to_string()), serde_yaml::to_value(vars).unwrap());
        }
        serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter(vec![(serde_yaml::Value::String("graphql".to_string()), serde_yaml::Value::Mapping(graphql_map))]))
      }
    };
    value.serialize(serializer)
  }
}

#[derive(Debug, Serialize)]
pub struct FormDataPart {
  pub key: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub value: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub file: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub content_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DrillAuth {
  #[serde(rename = "type")]
  pub auth_type: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub key: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub value: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none", rename = "in")]
  pub location: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub username: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub password: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub token: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub flow: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub token_url: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub client_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub client_secret: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub scope: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none", rename = "save_token_as")]
  pub save_token_as: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WithItemsRange {
  pub start: i64,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub step: Option<i64>,
  pub stop: i64,
}

#[allow(dead_code)]
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum WithItemsFromCsv {
  Simple(String),
  Detailed {
    file_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    quote_char: Option<String>,
  },
}

#[derive(Debug, Serialize)]
pub struct AssignItem {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  pub assign: AssignConfig,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub weight: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct AssignConfig {
  pub key: String,
  pub value: String,
}

#[derive(Debug, Serialize)]
pub struct AssertItem {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  pub assert: AssertConfig,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub weight: Option<u32>,
}

#[allow(dead_code)]
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum AssertConfig {
  Equals {
    key: String,
    value: String,
  },
  Status {
    #[serde(rename = "type")]
    _type: String,
    value: serde_yaml::Value,
  },
  Header {
    #[serde(rename = "type")]
    _type: String,
    key: String,
    value: String,
  },
  JsonPath {
    #[serde(rename = "type")]
    _type: String,
    key: String,
    value: String,
  },
  Duration {
    #[serde(rename = "type")]
    _type: String,
    value: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    operator: Option<String>,
  },
}

#[derive(Debug, Serialize)]
pub struct SaveItem {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  pub save: SaveConfig,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub weight: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct SaveConfig {
  pub source: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub jsonpath: Option<String>,
  pub key: String,
}

#[derive(Debug, Serialize)]
pub struct ExecItem {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  pub exec: ExecConfig,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub weight: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ExecConfig {
  pub command: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub assign: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DelayItem {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  pub delay: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub weight: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ForEachItem {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  pub for_each: ForEachConfig,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub weight: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ForEachConfig {
  pub items: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub item_key: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub index_key: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub shuffle: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub pick: Option<usize>,
  pub plan: Vec<PlanItem>,
}
