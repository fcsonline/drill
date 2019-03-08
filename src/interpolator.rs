use std::collections::HashMap;

use regex::{Regex, Captures};
use serde_json::Value;
use colored::*;
use yaml_rust::Yaml;

pub struct Interpolator<'a> {
  context: &'a HashMap<String, Yaml>,
  responses: &'a HashMap<String, Value>,
}

impl<'a> Interpolator<'a> {
  pub fn new(context: &'a HashMap<String, Yaml>, responses: &'a HashMap<String, Value>) -> Interpolator<'a> {
    Interpolator {
      context: context,
      responses: responses
    }
  }

  pub fn resolve(&self, url: &String) -> String {
    let re = Regex::new(r"\{\{ *([a-zA-Z\._]+[a-zA-Z\._0-9]*) *\}\}").unwrap();

    let result = re.replace_all(url.as_str(), |caps: &Captures| {
      let capture = &caps[1];

      if let Some(item) = self.resolve_context_interpolation(&capture) {
        return item.to_string();
      }

      if let Some(item) = self.resolve_responses_interpolation(&capture) {
        return item.to_string();
      }

      panic!("{} Unknown '{}' variable!", "WARNING!".yellow().bold(), &capture);
    }).to_string();

    if &result[..1] == "/" {
      match self.context.get("base") {
        Some(value) => {
          if let Some(vs) = value.as_str() {
            return vs.to_string() + &result;
          }

          panic!("{} Wrong type 'base' variable!", "WARNING!".yellow().bold());
        },
        _ => {
          panic!("{} Unknown 'base' variable!", "WARNING!".yellow().bold());
        }
      }
    } else {
      result
    }
  }

  // TODO: Refactor this function to support multiple levels
  fn resolve_responses_interpolation(&self, capture: &str) -> Option<String> {
    let cap_path: Vec<&str> = capture.split('.').collect();

    let (cap_root, cap_tail) = cap_path.split_at(1);

    match self.responses.get(cap_root[0]) {
      Some(value) => {
        return Some(value[cap_tail[0]].to_string());
      },
      _ => {
        None
      }
    }
  }

  // TODO: Refactor this function to support multiple levels
  fn resolve_context_interpolation(&self, capture: &str) -> Option<String> {
    let cap_path: Vec<&str> = capture.split('.').collect();

    let (cap_root, cap_tail) = cap_path.split_at(1);

    match self.context.get(cap_root[0]) {
      Some(value) => {
        if let Some(vs) = value.as_str() {
          return Some(vs.to_string());
        }

        if let Some(vi) = value.as_i64() {
          return Some(vi.to_string());
        }

        if let Some(vh) = value.as_hash() {
          let item_key = Yaml::String(cap_tail[0].to_string());

          match vh.get(&item_key){
            Some(value) => {
              if let Some(vs) = value.as_str() {
                return Some(vs.to_string());
              }

              if let Some(vi) = value.as_i64() {
                return Some(vi.to_string());
              }

              panic!("{} Unknown type for '{}' variable!", "WARNING!".yellow().bold(), &capture);
            },
            _ => {
              panic!("{} Unknown '{}' variable!", "WARNING!".yellow().bold(), &capture);
            }
          }
        }

        panic!("{} Unknown type for '{}' variable!", "WARNING!".yellow().bold(), &capture);
      },
      _ => {
        None
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::Value;
  use serde_json;

  #[test]
  fn interpolates_variables() {
    let mut context: HashMap<String, Yaml> = HashMap::new();
    let responses: HashMap<String, Value> = HashMap::new();

    context.insert(String::from("user_Id"), Yaml::String(String::from("12")));

    let interpolator = Interpolator{ context: &context, responses: &responses };
    let url = String::from("http://example.com/users/{{ user_Id }}/view/{{ user_Id }}");
    let interpolated = interpolator.resolve(&url);

    assert_eq!(interpolated, "http://example.com/users/12/view/12");
  }

  #[test]
  fn interpolates_responses() {
    let context: HashMap<String, Yaml> = HashMap::new();
    let mut responses: HashMap<String, Value> = HashMap::new();

    let data = String::from("{ \"bar\": 12 }");
    let value: serde_json::Value = serde_json::from_str(&data).unwrap();
    responses.insert(String::from("foo"), value);

    let interpolator = Interpolator{ context: &context, responses: &responses };
    let url = String::from("http://example.com/users/{{ foo.bar }}");
    let interpolated = interpolator.resolve(&url);

    assert_eq!(interpolated, "http://example.com/users/12");
  }

  #[test]
  fn interpolates_relatives() {
    let mut context: HashMap<String, Yaml> = HashMap::new();
    let responses: HashMap<String, Value> = HashMap::new();

    context.insert(String::from("base"), Yaml::String(String::from("http://example.com")));

    let interpolator = Interpolator{ context: &context, responses: &responses };
    let url = String::from("/users/1");
    let interpolated = interpolator.resolve(&url);

    assert_eq!(interpolated, "http://example.com/users/1");
  }

  #[test]
  #[should_panic]
  fn interpolates_missing_variable() {
    let context: HashMap<String, Yaml> = HashMap::new();
    let responses: HashMap<String, Value> = HashMap::new();

    let interpolator = Interpolator{ context: &context, responses: &responses };
    let url = String::from("/users/{{ userId }}");
    interpolator.resolve(&url);
  }

  #[test]
  #[should_panic]
  fn interpolates_missing_base() {
    let context: HashMap<String, Yaml> = HashMap::new();
    let responses: HashMap<String, Value> = HashMap::new();

    let interpolator = Interpolator{ context: &context, responses: &responses };
    let url = String::from("/users/1");
    interpolator.resolve(&url);
  }
}
