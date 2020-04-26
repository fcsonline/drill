use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

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
  if let Err(why) = file.read_to_string(&mut content) {
    panic!("couldn't read {}: {}", display, why);
  }

  content
}

// TODO: Try to split this fn into two
pub fn read_csv_file_as_yml(filepath: &str, quote: u8) -> yaml_rust::yaml::Array {
  // Create a path to the desired file
  let path = Path::new(filepath);
  let display = path.display();

  // Open the path in read-only mode, returns `io::Result<File>`
  let file = match File::open(&path) {
    Err(why) => panic!("couldn't open {}: {}", display, why),
    Ok(file) => file,
  };

  let mut rdr = csv::ReaderBuilder::new().has_headers(true).quote(quote).from_reader(file);

  let mut items = yaml_rust::yaml::Array::new();

  let headers = match rdr.headers() {
    Err(why) => panic!("error parsing header: {:?}", why),
    Ok(h) => h.clone(),
  };

  for result in rdr.records() {
    match result {
      Ok(record) => {
        let mut linked_hash_map = linked_hash_map::LinkedHashMap::new();

        for (i, header) in headers.iter().enumerate() {
          let item_key = yaml_rust::Yaml::String(header.to_string());
          let item_value = yaml_rust::Yaml::String(record.get(i).unwrap().to_string());

          linked_hash_map.insert(item_key, item_value);
        }

        items.push(yaml_rust::Yaml::Hash(linked_hash_map));
      }
      Err(e) => println!("error parsing header: {:?}", e),
    }
  }

  items
}
