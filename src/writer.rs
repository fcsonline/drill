use std::fs::File;
use std::io::{self, prelude::*};
use std::path::Path;

pub fn write_file(filepath: &str, content: String) -> Result<(), io::Error> {
    let path = Path::new(filepath);
    let display = path.display();

    let mut file = match File::create(path) {
        Ok(file) => file,
        Err(why) => return Err(io::Error::new(io::ErrorKind::Other, format!("couldn't create {}: {:?}", display, why))),
    };

    Ok(file.write_all(content.as_bytes())?)
}
