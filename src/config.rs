use colored::Colorize;
use linked_hash_map::LinkedHashMap;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use yaml_rust::{Yaml, YamlLoader};

use crate::benchmark::Context;
use crate::interpolator;
use crate::reader;

const NITERATIONS: i64 = 1;
const NRAMPUP: i64 = 0;

pub struct Config {
  pub base: String,
  pub concurrency: i64,
  pub iterations: i64,
  pub relaxed_interpolations: bool,
  pub no_check_certificate: bool,
  pub rampup: i64,
  pub quiet: bool,
  pub nanosec: bool,
  pub timeout: u64,
  pub verbose: bool,
  pub default_headers: HeaderMap,
  pub copy_headers: Vec<String>,
}

impl Config {
  pub fn new(path: &str, relaxed_interpolations: bool, no_check_certificate: bool, quiet: bool, nanosec: bool, timeout: u64, verbose: bool) -> Config {
    let config_file = reader::read_file(path);

    let config_docs = YamlLoader::load_from_str(config_file.as_str()).unwrap();
    let config_doc = &config_docs[0];

    let context: Context = Context::new();
    let interpolator = interpolator::Interpolator::new(&context);

    let iterations = read_i64_configuration(config_doc, &interpolator, "iterations", NITERATIONS);
    let concurrency = read_i64_configuration(config_doc, &interpolator, "concurrency", iterations);
    let rampup = read_i64_configuration(config_doc, &interpolator, "rampup", NRAMPUP);
    let base = read_str_configuration(config_doc, &interpolator, "base", "");
    let mut default_headers = HeaderMap::new();
    let hash = read_hashmap_configuration(config_doc, "default_headers", LinkedHashMap::new());
    for (key, val) in hash.iter() {
      if let Some(vs) = val.as_str() {
        default_headers.insert(
          HeaderName::from_bytes(key.as_str().unwrap().as_bytes()).unwrap(),
          HeaderValue::from_str(vs).unwrap(),
        );
      } else {
        panic!("{} Headers must be strings!!", "WARNING!".yellow().bold());
      }
    }
    let mut copy_headers = Vec::new();
    for v in read_list_configuration(config_doc, "copy_headers", Vec::new()).iter() {
      copy_headers.push(v.as_str().unwrap().to_string());
    };
    

    if concurrency > iterations {
      panic!("The concurrency can not be higher than the number of iterations")
    }

    Config {
      base,
      concurrency,
      iterations,
      relaxed_interpolations,
      no_check_certificate,
      rampup,
      quiet,
      nanosec,
      timeout,
      verbose,
      default_headers,
      copy_headers,
    }
  }
}

fn read_str_configuration(config_doc: &Yaml, interpolator: &interpolator::Interpolator, name: &str, default: &str) -> String {
  match config_doc[name].as_str() {
    Some(value) => {
      if value.contains('{') {
        interpolator.resolve(value, true)
      } else {
        value.to_owned()
      }
    }
    None => {
      if config_doc[name].as_str().is_some() {
        println!("Invalid {} value!", name);
      }

      default.to_owned()
    }
  }
}

fn read_hashmap_configuration(config_doc: &Yaml, name: &str, default: LinkedHashMap<Yaml, Yaml>) -> LinkedHashMap<Yaml, Yaml> {
  match config_doc[name].as_hash() {
    Some(value) => {
      value.clone()
    }
    None => {
      if config_doc[name].as_hash().is_some() {
        println!("Invalid {} value!", name);
      }

      default.to_owned()
    }
  }
}

fn read_list_configuration(config_doc: &Yaml, name: &str, default: Vec<Yaml>) -> Vec<Yaml> {
  match config_doc[name].as_vec() {
    Some(value) => {
      value.clone()
    }
    None => {
      if config_doc[name].as_vec().is_some() {
        println!("Invalid {} value!", name);
      }

      default.to_owned()
    }
  }
}

fn read_i64_configuration(config_doc: &Yaml, interpolator: &interpolator::Interpolator, name: &str, default: i64) -> i64 {
  let value = if let Some(value) = config_doc[name].as_i64() {
    Some(value)
  } else if let Some(key) = config_doc[name].as_str() {
    interpolator.resolve(key, false).parse::<i64>().ok()
  } else {
    None
  };

  match value {
    Some(value) => {
      if value < 0 {
        println!("Invalid negative {} value!", name);

        default
      } else {
        value
      }
    }
    None => {
      if config_doc[name].as_str().is_some() {
        println!("Invalid {} value!", name);
      }

      default
    }
  }
}
