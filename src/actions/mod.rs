mod assign;
mod request;

pub use self::assign::Assign;
pub use self::request::Request;

use std::collections::HashMap;

extern crate yaml_rust;
use self::yaml_rust::Yaml;

extern crate serde_json;
use self::serde_json::Value;

pub trait Runnable {
  fn execute(&self, _base_url: &String, context: &mut HashMap<String, Yaml>, responses: &mut HashMap<String, Value>);
}
