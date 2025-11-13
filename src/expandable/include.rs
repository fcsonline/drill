use std::io;
use std::path::Path;
use yaml_rust2::Yaml;

use crate::interpolator::INTERPOLATION_REGEX;

use crate::benchmark::Benchmark;
use crate::expandable::{self};
use crate::tags::Tags;

pub fn is_that_you(item: &Yaml) -> bool {
    item["include"].as_str().is_some()
}

pub fn expand(parent_path: &str, item: &Yaml, benchmark: &mut Benchmark, tags: &Tags) -> Result<(), io::Error> {
    let include_path = item["include"].as_str().unwrap();

    let regex = match INTERPOLATION_REGEX.as_ref() {
        Ok(regex) => regex,
        Err(err) => return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("Invalid regex: {}", err))),
    };

    if regex.is_match(include_path) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Interpolations not supported in 'include' property!"));
    }

    let include_filepath = Path::new(parent_path).with_file_name(include_path);
    let final_path = include_filepath.to_str().unwrap();

    expandable::expand_from_filepath(final_path, benchmark, None, tags)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::benchmark::Benchmark;
    use crate::expandable::include::{expand, is_that_you};
    use crate::tags::Tags;

    #[test]
    fn expand_include() {
        let text = "---\nname: Include comment\ninclude: comments.yml";
        let docs = yaml_rust2::YamlLoader::load_from_str(text).unwrap();
        let doc = &docs[0];
        let mut benchmark: Benchmark = Benchmark::new();

        let result = expand("example/benchmark.yml", doc, &mut benchmark, &Tags::new(None, None).unwrap());

        assert!(result.is_ok());
        assert!(is_that_you(doc));
        assert_eq!(benchmark.len(), 2);
    }

    #[test]
    fn invalid_expand() {
        let text = "---\nname: Include comment\ninclude: {{ memory }}.yml";
        let docs = yaml_rust2::YamlLoader::load_from_str(text);
        assert!(docs.is_err());
    }
}
