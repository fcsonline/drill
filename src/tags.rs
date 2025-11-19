use colored::Colorize;

use crate::parser::BenchmarkConfig;
use std::{collections::HashSet, io};

#[derive(Debug)]
pub struct Tags<'a> {
    pub tags: Option<HashSet<&'a str>>,
    pub skip_tags: Option<HashSet<&'a str>>,
}

impl<'a> Tags<'a> {
    pub fn new(tags_option: Option<&'a str>, skip_tags_option: Option<&'a str>) -> Result<Self, io::Error> {
        let tags: Option<HashSet<&str>> = tags_option.map(|m| m.split(',').map(|s| s.trim()).collect());
        let skip_tags: Option<HashSet<&str>> = skip_tags_option.map(|m| m.split(',').map(|s| s.trim()).collect());

        if let (Some(t), Some(s)) = (&tags, &skip_tags) {
            if !t.is_disjoint(s) {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "`tags` and `skip-tags` must not contain the same values!"));
            }
        }

        Ok(Tags {
            tags,
            skip_tags,
        })
    }

    pub fn is_excluded(&self, tags: &Vec<String>) -> bool {
        if let Some(t) = &self.tags {
            if tags.contains(&"never".to_string()) && !t.contains(&"never") {
                return true;
            }

            if !t.is_disjoint(&tags.clone().iter().map(|s| s.as_str()).collect::<HashSet<&str>>()) {
                return false;
            }
        }
        if tags.contains(&"always".to_string()) {
            return false;
        }
        if tags.contains(&"never".to_string()) {
            return true;
        }
        return false;
    }
}

pub fn list_benchmark_file_tasks(benchmark_config: BenchmarkConfig, app_tags: &Tags) -> Result<(), io::Error> {
    let items = benchmark_config.plan;

    println!();

    if let Some(tags) = &app_tags.tags {
        let mut tags: Vec<_> = tags.iter().collect();
        tags.sort();
        println!("{:width$} {:width2$?}", "Tags".green(), &tags, width = 15, width2 = 25);
    }
    if let Some(tags) = &app_tags.skip_tags {
        let mut tags: Vec<_> = tags.iter().collect();
        tags.sort();
        println!("{:width$} {:width2$?}", "Skip-Tags".green(), &tags, width = 15, width2 = 25);
    }

    if items.is_empty() {
        return Err(io::Error::new(io::ErrorKind::Other, "No items"));
    }

    for item in items {
        if app_tags.is_excluded(item.tags.as_ref().unwrap()) {
            continue;
        }
        println!("{:?}", item);
    }

    Ok(())
}

pub fn list_benchmark_file_tags(benchmark_config: BenchmarkConfig) -> Result<(), io::Error> {
    let items = benchmark_config.plan;

    println!();

    if items.is_empty() {
        return Err(io::Error::new(io::ErrorKind::Other, "No items"));
    }
    let mut tags: HashSet<&str> = HashSet::new();
    for item in items {
        if let Some(item_tags) = item.tags {
            tags.extend(item_tags.iter().map(|t| t.as_str()));
        }
    }

    let mut tags: Vec<_> = tags.into_iter().collect();
    tags.sort_unstable();
    println!("{:width$} {:?}", "Tags".green(), &tags, width = 15);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn str_to_yaml(text: &str) -> Yaml {
        let mut docs = yaml_rust2::YamlLoader::load_from_str(text).unwrap();
        docs.remove(0)
    }

    fn prepare_default_item() -> Yaml {
        str_to_yaml("---\nname: foo\nrequest:\n  url: /\ntags:\n  - tag1\n  - tag2")
    }

    #[test]
    fn same_tags_and_skip_tags() {
        let result = Tags::new(Some("tag1"), Some("tag1"));
        assert!(result.is_err());
    }

    #[test]
    fn empty_tags_both() {
        let item = str_to_yaml("---\nname: foo\nrequest:\n  url: /");
        let tags = Tags::new(None, None);
        assert!(!tags.unwrap().should_skip_item(&item));
    }

    #[test]
    fn empty_tags() {
        let tags = Tags::new(None, None);
        assert!(!tags.unwrap().should_skip_item(&prepare_default_item()));
    }

    #[test]
    fn tags_contains() {
        let tags = Tags::new(Some("tag1"), None);
        assert!(!tags.unwrap().should_skip_item(&prepare_default_item()));
    }

    #[test]
    fn tags_contains_second() {
        let tags = Tags::new(Some("tag2"), None);
        assert!(!tags.unwrap().should_skip_item(&prepare_default_item()));
    }

    #[test]
    fn tags_contains_both() {
        let tags = Tags::new(Some("tag1,tag2"), None);
        assert!(!tags.unwrap().should_skip_item(&prepare_default_item()));
    }

    #[test]
    fn tags_not_contains() {
        let tags = Tags::new(Some("tag99"), None);
        assert!(tags.unwrap().should_skip_item(&prepare_default_item()));
    }

    #[test]
    fn skip_tags_not_contains() {
        let tags = Tags::new(None, Some("tag99"));
        assert!(!tags.unwrap().should_skip_item(&prepare_default_item()));
    }

    #[test]
    fn skip_tags_contains() {
        let tags = Tags::new(None, Some("tag1"));
        assert!(tags.unwrap().should_skip_item(&prepare_default_item()));
    }

    #[test]
    fn skip_tags_contains_second() {
        let tags = Tags::new(None, Some("tag2"));
        assert!(tags.unwrap().should_skip_item(&prepare_default_item()));
    }

    #[test]
    fn tags_contains_but_also_skip_tags_contains() {
        let tags = Tags::new(Some("tag1"), Some("tag2"));
        assert!(tags.unwrap().should_skip_item(&prepare_default_item()));
    }

    #[test]
    fn never_skipped_by_default() {
        let item = str_to_yaml("---\nname: foo\nrequest:\n  url: /\ntags:\n  - never\n  - tag2");
        let tags = Tags::new(None, None);
        assert!(tags.unwrap().should_skip_item(&item));
    }

    #[test]
    fn never_tag_skipped_even_when_other_tag_included() {
        let item = str_to_yaml("---\nname: foo\nrequest:\n  url: /\ntags:\n  - never\n  - tag2");
        let tags = Tags::new(Some("tag2"), None);
        assert!(tags.unwrap().should_skip_item(&item));
    }

    #[test]
    fn include_never_tag() {
        let item = str_to_yaml("---\nname: foo\nrequest:\n  url: /\ntags:\n  - never\n  - tag2");
        let tags = Tags::new(Some("never"), None);
        assert!(!tags.unwrap().should_skip_item(&item));
    }

    #[test]
    fn always_tag_included_by_default() {
        let item = str_to_yaml("---\nname: foo\nrequest:\n  url: /\ntags:\n  - always\n  - tag2");
        let tags = Tags::new(Some("tag99"), None);
        assert!(!tags.unwrap().should_skip_item(&item));
    }

    #[test]
    fn skip_always_tag() {
        let item = str_to_yaml("---\nname: foo\nrequest:\n  url: /\ntags:\n  - always\n  - tag2");
        let tags = Tags::new(None, Some("always"));
        assert!(tags.unwrap().should_skip_item(&item));
    }
}
