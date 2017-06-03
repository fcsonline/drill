use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use std::collections::BTreeMap;

extern crate csv;
extern crate yaml_rust;

pub fn read_file(filepath: &str) -> String {
  // Create a path to the desired file
  let path = Path::new(filepath);
  let display = path.display();

  // Open the path in read-only mode, returns `io::Result<File>`
  let mut file = match File::open(&path) {
    Err(why) => panic!("couldn't open {}: {}", display, why),
    Ok(file) => file,
  };

  // Read the file contents into a string, returns `io::Result<usize>`
  let mut content = String::new();
  match file.read_to_string(&mut content) {
    Err(why) => panic!("couldn't read {}: {}", display, why),
    Ok(_) => {},
  }

  content
}

// TODO: Try to split this fn into two
pub fn read_csv_file_as_yml(filepath: &str) -> yaml_rust::yaml::Array {
  // Create a path to the desired file
  let path = Path::new(filepath);
  let display = path.display();

  // Open the path in read-only mode, returns `io::Result<File>`
  let file = match File::open(&path) {
    Err(why) => panic!("couldn't open {}: {}", display, why),
    Ok(file) => file,
  };

  let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(file);

  let mut items = yaml_rust::yaml::Array::new();

  let headers = match rdr.headers() {
    Err(why) => panic!("error parsing header: {:?}", why),
    Ok(h) => h.clone(),
  };

  for result in rdr.records() {
      match result {
        Ok(record) => {
          let mut item_tree = BTreeMap::new();

          for (i, header) in headers.iter().enumerate() {
            let item_key = yaml_rust::Yaml::String(header.to_string());
            let item_value = yaml_rust::Yaml::String(record.get(i).unwrap().to_string());

            item_tree.insert(item_key, item_value);
          }

          items.push(yaml_rust::Yaml::Hash(item_tree));
        },
        Err(e) => println!("error parsing header: {:?}", e),
      }
  }

  items
}
