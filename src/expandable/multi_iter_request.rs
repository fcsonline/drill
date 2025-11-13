use std::convert::TryInto;
use std::io;

use rand::seq::SliceRandom;
use rand::thread_rng;
use yaml_rust2::Yaml;

use crate::interpolator::INTERPOLATION_REGEX;

use crate::actions::Request;
use crate::benchmark::Benchmark;

pub fn is_that_you(item: &Yaml) -> bool {
    item["request"].as_hash().is_some() && item["with_items_range"].as_hash().is_some()
}

pub fn expand(item: &Yaml, benchmark: &mut Benchmark) -> Result<(), io::Error> {
    if let Some(with_iter_items) = item["with_items_range"].as_hash() {
        let init = Yaml::Integer(1);
        let lstart = Yaml::String("start".into());
        let lstep = Yaml::String("step".into());
        let lstop = Yaml::String("stop".into());

        let vstart: &Yaml = match with_iter_items.get(&lstart) {
            Some(value) => value,
            None => return Err(io::Error::new(io::ErrorKind::InvalidInput, "Start property must be an integer")),
        };
        let vstep: &Yaml = with_iter_items.get(&lstep).unwrap_or(&init);
        let vstop: &Yaml = match with_iter_items.get(&lstop) {
            Some(value) => value,
            None => return Err(io::Error::new(io::ErrorKind::InvalidInput, "Stop property must be an integer")),
        };

        let start: &str = vstart.as_str().unwrap_or("");
        let step: &str = vstep.as_str().unwrap_or("");
        let stop: &str = vstop.as_str().unwrap_or("");

        let regex = match INTERPOLATION_REGEX.as_ref() {
            Ok(regex) => regex,
            Err(err) => return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("Invalid regex: {}", err))),
        };

        if regex.is_match(start) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Interpolations not supported in 'start' property!"));
        }

        if regex.is_match(step) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Interpolations not supported in 'step' property!"));
        }

        if regex.is_match(stop) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Interpolations not supported in 'stop' property!"));
        }

        let start: i64 = match vstart.as_i64() {
            Some(start) => start,
            None => return Err(io::Error::new(io::ErrorKind::InvalidInput, "Start needs to be a number")),
        };
        let step: i64 = match vstep.as_i64() {
            Some(step) => step,
            None => return Err(io::Error::new(io::ErrorKind::InvalidInput, "Step needs to be a number")),
        };
        let stop: i64 = match vstop.as_i64() {
            Some(stop) => stop,
            None => return Err(io::Error::new(io::ErrorKind::InvalidInput, "Stop needs to be a number")),
        };

        let stop = stop + 1; // making stop inclusive

        if stop > start && start > 0 {
            let mut with_items: Vec<i64> = (start..stop).step_by(step as usize).collect();

            if let Some(shuffle) = item["shuffle"].as_bool() {
                if shuffle {
                    let mut rng = thread_rng();
                    with_items.shuffle(&mut rng);
                }
            }

            if let Some(pick) = item["pick"].as_i64() {
                match pick.try_into() {
                    Ok(pick) => with_items.truncate(pick),
                    Err(_) => return Err(io::Error::new(io::ErrorKind::InvalidInput, "Pick needs to be a number")),
                }
            }

            for (index, value) in with_items.iter().enumerate() {
                let index = index as u32;

                benchmark.push(Box::new(Request::new(item, Some(Yaml::Integer(*value)), Some(index))?));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_multi_range() {
        let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\nwith_items_range:\n  start: 2\n  step: 2\n  stop: 20";
        let docs = yaml_rust2::YamlLoader::load_from_str(text).unwrap();
        let doc = &docs[0];
        let mut benchmark: Benchmark = Benchmark::new();

        let result = expand(doc, &mut benchmark);
        assert!(result.is_ok());

        assert!(is_that_you(doc));
        assert_eq!(benchmark.len(), 10);
    }

    #[test]
    fn expand_multi_range_should_limit_requests_using_the_pick_option() {
        let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\npick: 3\nwith_items_range:\n  start: 2\n  step: 2\n  stop: 20";
        let docs = yaml_rust2::YamlLoader::load_from_str(text).unwrap();
        let doc = &docs[0];
        let mut benchmark: Benchmark = Benchmark::new();

        let result = expand(doc, &mut benchmark);
        assert!(result.is_ok());

        assert!(is_that_you(doc));
        assert_eq!(benchmark.len(), 3);
    }

    #[test]
    fn invalid_expand() {
        let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\nwith_items_range:\n  start: 1\n  step: 2\n  stop: foo";
        let docs = yaml_rust2::YamlLoader::load_from_str(text).unwrap();
        let doc = &docs[0];
        let mut benchmark: Benchmark = Benchmark::new();

        let result = expand(doc, &mut benchmark);
        assert!(result.is_err());
    }

    #[test]
    fn runtime_expand() {
        let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\nwith_items_range:\n  start: 1\n  step: 2\n  stop: \"{{ memory }}\"";
        let docs = yaml_rust2::YamlLoader::load_from_str(text).unwrap();
        let doc = &docs[0];
        let mut benchmark: Benchmark = Benchmark::new();

        let result = expand(doc, &mut benchmark);
        assert!(result.is_err());
    }
}
