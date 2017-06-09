use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

pub fn write_file(filepath: &str, content: String) {
  let path = Path::new(filepath);
  let display = path.display();

  let mut file = match File::create(&path) {
    Err(why) => panic!("couldn't create {}: {:?}", display, why),
    Ok(file) => file,
  };

  match file.write_all(content.as_bytes()) {
    Err(why) => panic!("couldn't write to {}: {:?}", display, why),
    Ok(_) => {},
  }
}
