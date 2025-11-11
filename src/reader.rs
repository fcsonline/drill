use std::fs::File;
use std::io::{self, prelude::*, BufReader};
use std::path::Path;

use hashlink::LinkedHashMap;
use yaml_rust2::{yaml, Yaml};

pub fn read_file(filepath: &str) -> Result<String, io::Error> {
    // Create a path to the desired file
    let path = Path::new(filepath);

    // Open the path in read-only mode, returns `io::Result<File>`
    let mut file = File::open(path)?;

    // Read the file contents into a string, returns `io::Result<usize>`
    let mut content = String::new();
    let _ = file.read_to_string(&mut content)?;

    Ok(content)
}

pub fn read_file_as_yml(filepath: &str) -> Result<Vec<Yaml>, io::Error> {
    let content = read_file(filepath)?;

    match yaml_rust2::YamlLoader::load_from_str(content.as_str()) {
        Ok(yaml_str) => Ok(yaml_str),
        Err(err) => Err(io::Error::new(io::ErrorKind::Other, format!("Failed to parse YAML: {}", err))),
    }
}

pub fn read_yaml_doc_accessor<'a>(doc: &'a Yaml, accessor: Option<&str>) -> Result<&'a Vec<Yaml>, io::Error> {
    if let Some(accessor_id) = accessor {
        match doc[accessor_id].as_vec() {
            Some(items) => Ok(items),
            None => Err(io::Error::new(io::ErrorKind::NotFound, format!("Node missing on config: {}", accessor_id))),
        }
    } else {
        doc.as_vec().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No root node found"))
    }
}

pub fn read_file_as_yml_array(filepath: &str) -> Result<yaml::Array, io::Error> {
    let path = Path::new(filepath);

    let file = File::open(path)?;

    let reader = BufReader::new(file);
    let mut items = yaml::Array::new();
    for line in reader.lines() {
        match line {
            Ok(text) => {
                items.push(Yaml::String(text));
            }
            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, format!("Failed to read line: {}", e))),
        }
    }

    Ok(items)
}

// TODO: Try to split this fn into two
pub fn read_csv_file_as_yml(filepath: &str, quote: u8) -> Result<yaml::Array, io::Error> {
    // Create a path to the desired file
    let path = Path::new(filepath);

    // Open the path in read-only mode, returns `io::Result<File>`
    let file = File::open(path)?;

    let mut rdr = csv::ReaderBuilder::new().has_headers(true).quote(quote).from_reader(file);

    let mut items = yaml::Array::new();

    let headers = rdr.headers()?.clone();

    for result in rdr.records() {
        match result {
            Ok(record) => {
                let mut linked_hash_map = LinkedHashMap::new();

                for (i, header) in headers.iter().enumerate() {
                    let item_key = Yaml::String(header.to_string());
                    let item_value = Yaml::String(record.get(i).unwrap().to_string());

                    linked_hash_map.insert(item_key, item_value);
                }

                items.push(Yaml::Hash(linked_hash_map));
            }
            Err(e) => eprintln!("error parsing header: {e:?}"),
        }
    }

    Ok(items)
}
