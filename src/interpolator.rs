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

    let kaka = match context.get(cap_root[0]) {
      Some(value) => {
        let vs = value.as_str();
        let vi = value.as_i64();
        let vh = value.as_hash();

        if vs.is_some() {
          return vs.unwrap().to_string();
        }

        if vi.is_some() {
          return vi.unwrap().to_string();
        }

        if vh.is_some() {
          let item_key = yaml_rust::Yaml::String(cap_tail[0].to_string());

          return match vh.unwrap().get(&item_key){
            Some(value) => {
              let vs = value.as_str();
              let vi = value.as_i64();

              if vs.is_some() {
                return vs.unwrap().to_string();
              }

              if vi.is_some() {
                return vi.unwrap().to_string();
              }

              "wat".to_string()
            },
            _ => "hhh".to_string()
          }
        }

        "???".to_string()
      },
      _ => {
        match responses.get(&caps[1]) {
          Some(value) => "lol".to_string(),
          _ => {
            println!("{} Unknown '{}' variable!", "WARNING!".yellow().bold(), &caps[1]);
            "wat".to_string()
          }
        }
      }
    };

    kaka
  });

  result.to_string()
}
