use std::collections::HashMap;

extern crate regex;
use self::regex::{Regex, Captures};

extern crate serde_json;
use self::serde_json::Value;

extern crate colored;
use self::colored::*;

extern crate yaml_rust;
use self::yaml_rust::Yaml;

pub fn resolve_interpolations(url: &String, context: &HashMap<&str, Yaml>, responses: &HashMap<String, Value>) -> String {
  let re = Regex::new(r"\{\{ *([a-z\.]+) *\}\}").unwrap();

  let result = re.replace(url.as_str(), |caps: &Captures| {
    let cap_path: Vec<&str> = caps[1].split(".").collect();

    let (cap_root, cap_tail) = cap_path.split_at(1);

    match context.get(cap_root[0]) {
      Some(value) => {
        if let Some(vs) = value.as_str() {
          return vs.to_string();
        }

        if let Some(vi) = value.as_i64() {
          return vi.to_string();
        }

        if let Some(vh) = value.as_hash() {
          let item_key = yaml_rust::Yaml::String(cap_tail[0].to_string());

          match vh.get(&item_key){
            Some(value) => {
              if let Some(vs) = value.as_str() {
                return vs.to_string();
              }

              if let Some(vi) = value.as_i64() {
                return vi.to_string();
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
        match responses.get(&caps[1]) {
          Some(_value) => "lol".to_string(),
          _ => {
            panic!("{} Unknown '{}' variable!", "WARNING!".yellow().bold(), &caps[1]);
          }
        }
      }
    }
  });

  result.to_string()
}
