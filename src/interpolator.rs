use std::collections::HashMap;

use colored::*;
use regex::{Captures, Regex};
use serde_json::Value;
use yaml_rust::Yaml;

pub struct Interpolator<'a> {
  context: &'a HashMap<String, Yaml>,
  responses: &'a HashMap<String, Value>,
  regexp: Regex,
}

impl<'a> Interpolator<'a> {
  pub fn new(context: &'a HashMap<String, Yaml>, responses: &'a HashMap<String, Value>) -> Interpolator<'a> {
    Interpolator {
      context: context,
      responses: responses,
      regexp: Regex::new(r"\{\{ *([a-zA-Z\._]+[a-zA-Z\._0-9]*) *\}\}").unwrap(),
    }
  }

  pub fn resolve(&self, url: &String) -> String {
    self
      .regexp
      .replace_all(url.as_str(), |caps: &Captures| {
        let capture = &caps[1];

        if let Some(item) = self.resolve_context_interpolation(capture.split('.').collect()) {
          return item.to_string();
        }

        if let Some(item) = self.resolve_responses_interpolation(capture.split('.').collect()) {
          return item.to_string();
        }

        eprintln!("{} Unknown '{}' variable!", "WARNING!".yellow().bold(), &capture);

        "".to_string()
      })
      .to_string()
  }

  fn resolve_responses_interpolation(&self, cap_path: Vec<&str>) -> Option<String> {
    let (cap_root, cap_tail) = cap_path.split_at(1);

    cap_tail
      .into_iter()
      .fold(self.responses.get(cap_root[0]), |json, k| match json {
        Some(json) => json.get(k),
        _ => None,
      })
      .map(|value| {
        if value.is_string() {
          String::from(value.as_str().unwrap())
        } else {
          value.to_string()
        }
      })
  }

  fn resolve_context_interpolation(&self, cap_path: Vec<&str>) -> Option<String> {
    let (cap_root, cap_tail) = cap_path.split_at(1);

    cap_tail
      .into_iter()
      .fold(self.context.get(cap_root[0]), |yaml, k| match yaml {
        Some(yaml) => match yaml.as_hash() {
          Some(yaml) => yaml.get(&Yaml::from_str(k)),
          _ => None,
        },
        _ => None,
      })
      .map(|val| val.as_str().map(String::from).or(val.as_i64().map(|x| x.to_string())).unwrap_or("".to_string()))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json;
  use serde_json::Value;

  #[test]
  fn interpolates_variables() {
    let mut context: HashMap<String, Yaml> = HashMap::new();
    let responses: HashMap<String, Value> = HashMap::new();

    context.insert(String::from("user_Id"), Yaml::String(String::from("12")));

    let interpolator = Interpolator::new(&context, &responses);
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

    let interpolator = Interpolator::new(&context, &responses);
    let url = String::from("http://example.com/users/{{ foo.bar }}");
    let interpolated = interpolator.resolve(&url);

    assert_eq!(interpolated, "http://example.com/users/12");
  }

  #[test]
  #[should_panic]
  fn interpolates_missing_variable() {
    let context: HashMap<String, Yaml> = HashMap::new();
    let responses: HashMap<String, Value> = HashMap::new();

    let interpolator = Interpolator::new(&context, &responses);
    let url = String::from("/users/{{ userId }}");
    interpolator.resolve(&url);
  }

  #[test]
  fn interpolates_numnamed_variables() {
    let mut context: HashMap<String, Yaml> = HashMap::new();
    let responses: HashMap<String, Value> = HashMap::new();

    context.insert(String::from("zip5"), Yaml::String(String::from("90210")));

    let interpolator = Interpolator::new(&context, &responses);
    let url = String::from("http://example.com/postalcode/{{ zip5 }}/view/{{ zip5 }}");
    let interpolated = interpolator.resolve(&url);

    assert_eq!(interpolated, "http://example.com/postalcode/90210/view/90210");
  }

  #[test]
  fn interpolates_bad_numnamed_variable_names() {
    let mut context: HashMap<String, Yaml> = HashMap::new();
    let responses: HashMap<String, Value> = HashMap::new();

    context.insert(String::from("5digitzip"), Yaml::String(String::from("90210")));

    let interpolator = Interpolator::new(&context, &responses);
    let url = String::from("http://example.com/postalcode/{{ 5digitzip }}/view/{{ 5digitzip }}");
    let interpolated = interpolator.resolve(&url);

    assert_eq!(interpolated, "http://example.com/postalcode/{{ 5digitzip }}/view/{{ 5digitzip }}");
  }
}
