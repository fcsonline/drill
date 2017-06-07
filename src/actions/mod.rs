mod assign;
mod request;

pub use self::assign::Assign;
pub use self::request::Request;

use std::collections::HashMap;

use yaml_rust::Yaml;
use serde_json::Value;

pub trait Runnable {
  fn execute(&self, _base_url: &String, context: &mut HashMap<String, Yaml>, responses: &mut HashMap<String, Value>);
}
