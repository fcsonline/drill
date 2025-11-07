pub mod include;

mod multi_csv_request;
mod multi_file_request;
mod multi_iter_request;
mod multi_request;

use std::io;

use yaml_rust2::Yaml;

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
