pub mod include;

mod multi_csv_request;
mod multi_file_request;
mod multi_iter_request;
mod multi_request;

use std::io;

use yaml_rust2::{Yaml, YamlEmitter};

use crate::{actions, benchmark::Benchmark, reader, tags::Tags};

pub fn expand_from_filepath(parent_path: &str, benchmark: &mut Benchmark, accessor: Option<&str>, tags: &Tags) -> Result<(), io::Error> {
    let docs = reader::read_file_as_yml(parent_path)?;
    let items = reader::read_yaml_doc_accessor(&docs[0], accessor)?;

    for item in items {
        if include::is_that_you(item) {
            include::expand(parent_path, item, benchmark, tags)?;

            continue;
        }

        if tags.should_skip_item(item) {
            continue;
        }

        if multi_request::is_that_you(item) {
            multi_request::expand(item, benchmark)?;
        } else if multi_iter_request::is_that_you(item) {
            multi_iter_request::expand(item, benchmark)?;
        } else if multi_csv_request::is_that_you(item) {
            multi_csv_request::expand(parent_path, item, benchmark)?;
        } else if multi_file_request::is_that_you(item) {
            multi_file_request::expand(parent_path, item, benchmark)?;
        } else if actions::Delay::is_that_you(item) {
            benchmark.push(Box::new(actions::Delay::new(item, None)?));
        } else if actions::Exec::is_that_you(item) {
            benchmark.push(Box::new(actions::Exec::new(item, None)?));
        } else if actions::Assign::is_that_you(item) {
            benchmark.push(Box::new(actions::Assign::new(item, None)?));
        } else if actions::Assert::is_that_you(item) {
            benchmark.push(Box::new(actions::Assert::new(item, None)?));
        } else if actions::Request::is_that_you(item) {
            benchmark.push(Box::new(actions::Request::new(item, None, None)?));
        } else {
            let mut out_str = String::new();
            let mut emitter = YamlEmitter::new(&mut out_str);
            emitter.dump(item).unwrap();
            return Err(io::Error::new(io::ErrorKind::Other, format!("Unknown node:\n\n{}\n\n", out_str)));
        }
    }

    Ok(())
}

pub fn pick(item: &Yaml, with_items: &[Yaml]) -> Result<usize, io::Error> {
    match item["pick"].as_i64() {
        Some(value) => {
            if value.is_negative() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("pick option should not be negative, but was {}", value)));
            } else if value as usize > with_items.len() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("pick option should not be greater than the provided items, but was {}", value)));
            } else {
                Ok(value as usize)
            }
        }
        None => Ok(with_items.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod pick {
        use super::*;

        #[test]
        fn should_return_the_configured_value() {
            let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\npick: 2\nwith_items:\n  - 1\n  - 2\n  - 3";
            let item = &yaml_rust2::YamlLoader::load_from_str(text).unwrap()[0];
            let pick = pick(item, item["with_items"].as_vec().unwrap());

            assert!(matches!(pick, Ok(2)));
        }

        #[test]
        fn should_return_the_with_items_length_if_unset() {
            let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\nwith_items:\n  - 1\n  - 2\n  - 3";
            let item = &yaml_rust2::YamlLoader::load_from_str(text).unwrap()[0];
            let pick = pick(item, item["with_items"].as_vec().unwrap());

            assert!(matches!(pick, Ok(3)));
        }

        #[test]
        fn should_return_an_error_for_negative_values() {
            let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\npick: -1\nwith_items:\n  - 1\n  - 2\n  - 3";
            let item = &yaml_rust2::YamlLoader::load_from_str(text).unwrap()[0];
            let pick = pick(item, item["with_items"].as_vec().unwrap());

            assert!(matches!(pick, Err(e) if e.to_string() == "pick option should not be negative, but was -1"));
        }

        #[test]
        fn should_return_an_error_for_values_greater_than_the_items_list() {
            let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\npick: 4\nwith_items:\n  - 1\n  - 2\n  - 3";
            let item = &yaml_rust2::YamlLoader::load_from_str(text).unwrap()[0];
            let pick = pick(item, item["with_items"].as_vec().unwrap());

            assert!(matches!(pick, Err(e) if e.to_string() == "pick option should not be greater than the provided items, but was 4"));
        }
    }
}
