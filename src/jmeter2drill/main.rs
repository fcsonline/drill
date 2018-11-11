extern crate colored;
extern crate yaml_rust;
extern crate sxd_document;
extern crate sxd_xpath;

use std::process;
use sxd_document::parser;
use sxd_xpath::evaluate_xpath;
use std::collections::BTreeMap;
use yaml_rust::{Yaml, YamlEmitter};

fn main() {
  let input = include_str!("jmeter.jmx");
  let package = parser::parse(input).expect("failed to parse XML");
  let document = package.as_document();

  let value1 = evaluate_xpath(&document, "//ThreadGroup/elementProp/intProp[@name='LoopController.loops']").expect("XPath evaluation failed");
  let value2 = evaluate_xpath(&document, "//ThreadGroup/stringProp[@name='ThreadGroup.num_threads']").expect("XPath evaluation failed");

  let mut doc = BTreeMap::new();
  let mut plan = Vec::new();

  doc.insert(
    Yaml::String("iterations".to_string()),
    Yaml::Real(value1.into_string())
  );

  doc.insert(
    Yaml::String("threads".to_string()),
    Yaml::Real(value2.into_string())
  );

  let mut req = BTreeMap::new();

  req.insert(
    Yaml::String("name".to_string()),
    Yaml::String("kaka".to_string())
  );

  req.insert(
    Yaml::String("request".to_string()),
    Yaml::String("kaka".to_string())
  );

  plan.push(Yaml::Hash(req));


  let mut req1 = BTreeMap::new();

  req1.insert(
    Yaml::String("name".to_string()),
    Yaml::String("kaka".to_string())
  );

  req1.insert(
    Yaml::String("request".to_string()),
    Yaml::String("kaka".to_string())
  );

  plan.push(Yaml::Hash(req1));

  let plan2 = evaluate_xpath(&document, "//HTTPSampler").expect("XPath evaluation failed");
  println!("FCS: {:?}", plan2.array());

  doc.insert(
    Yaml::String("plan".to_string()),
    Yaml::Array(plan)
  );

  let mut out_str = String::new();
  {
      let mut emitter = YamlEmitter::new(&mut out_str);
      emitter.dump(&Yaml::Hash(doc)).unwrap(); // dump the YAML object to a String
  }
  println!("{}", out_str);

  process::exit(0)
}
