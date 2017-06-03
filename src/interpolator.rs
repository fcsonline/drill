use std::collections::HashMap;

extern crate regex;
use self::regex::{Regex, Captures};

extern crate serde_json;
use self::serde_json::Value;

extern crate colored;
use self::colored::*;

extern crate yaml_rust;
use self::yaml_rust::Yaml;

pub struct Interpolator<'a> {
  base_url: &'a String,
  context: &'a HashMap<&'a str, Yaml>,
  responses: &'a HashMap<String, Value>,
}

impl<'a> Interpolator<'a> {
  pub fn new(base_url: &'a String, context: &'a HashMap<&'a str, Yaml>, responses: &'a HashMap<String, Value>) -> Interpolator<'a> {
    Interpolator {
      base_url: base_url,
      context: context,
      responses: responses
    }
  }

  pub fn resolve(&self, url: &String) -> String {
    let re = Regex::new(r"\{\{ *([a-z\.]+) *\}\}").unwrap();

    let result = re.replace(url.as_str(), |caps: &Captures| {

      if let Some(item) = self.resolve_context_interpolation(&caps) {
        return item.to_string();
      }

      if let Some(item) = self.resolve_responses_interpolation(&caps) {
        return item.to_string();
      }

      panic!("{} Unknown '{}' variable!", "WARNING!".yellow().bold(), &caps[1]);
    });

    self.base_url.to_string() + &result
  }

  fn resolve_responses_interpolation(&self, caps: &Captures) -> Option<String> {
    match self.responses.get(&caps[1]) {
      Some(_value) => {
        // TODO
        None
      },
      _ => {
        None
      }
    }
  }

  fn resolve_context_interpolation(&self, caps: &Captures) -> Option<String> {
    let cap_path: Vec<&str> = caps[1].split(".").collect();

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
          let item_key = yaml_rust::Yaml::String(cap_tail[0].to_string());

          match vh.get(&item_key){
            Some(value) => {
              if let Some(vs) = value.as_str() {
                return Some(vs.to_string());
              }

              if let Some(vi) = value.as_i64() {
                return Some(vi.to_string());
              }

              panic!("{} Unknown type for '{}' variable!", "WARNING!".yellow().bold(), &caps[1]);
            },
            _ => {
              panic!("{} Unknown '{}' variable!", "WARNING!".yellow().bold(), &caps[1]);
            }
          }
        }

        panic!("{} Unknown type for '{}' variable!", "WARNING!".yellow().bold(), &caps[1]);
      },
      _ => {
        None
      }
    }
  }
}
