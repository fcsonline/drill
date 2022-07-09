use colored::*;
use lazy_static::lazy_static;
use regex::{Captures, Regex};

use crate::benchmark::Context;

static INTERPOLATION_PREFIX: &str = "{{";
static INTERPOLATION_SUFFIX: &str = "}}";

lazy_static! {
  pub static ref INTERPOLATION_REGEX: Regex = {
    let regexp = format!("{}{}{}", regex::escape(INTERPOLATION_PREFIX), r" *([a-zA-Z\-\._]+[a-zA-Z\-\._0-9]*) *", regex::escape(INTERPOLATION_SUFFIX));

    Regex::new(regexp.as_str()).unwrap()
  };
}

pub struct Interpolator<'a> {
  context: &'a Context,
}

impl<'a> Interpolator<'a> {
  pub fn new(context: &'a Context) -> Interpolator<'a> {
    Interpolator {
      context,
    }
  }

  pub fn resolve(&self, url: &str, strict: bool) -> String {
    INTERPOLATION_REGEX
      .replace_all(url, |caps: &Captures| {
        let capture = &caps[1];

        if let Some(item) = self.resolve_context_interpolation(capture.split('.').collect()) {
          return item;
        }

        if let Some(item) = self.resolve_environment_interpolation(capture) {
          return item;
        }

        if strict {
          panic!("Unknown '{}' variable!", &capture);
        }

        eprintln!("{} Unknown '{}' variable!", "WARNING!".yellow().bold(), &capture);

        "".to_string()
      })
      .to_string()
  }

  fn resolve_environment_interpolation(&self, value: &str) -> Option<String> {
    match std::env::vars().find(|tuple| tuple.0 == value) {
      Some(tuple) => Some(tuple.1),
      _ => None,
    }
  }

  fn resolve_context_interpolation(&self, cap_path: Vec<&str>) -> Option<String> {
    let (cap_root, cap_tail) = cap_path.split_at(1);

    cap_tail
      .iter()
      .fold(self.context.get(cap_root[0]), |json, k| match json {
        Some(json) => json.get(k),
        _ => None,
      })
      .map(|value| {
        if value.is_string() {
          String::from(value.as_str().unwrap())
        } else {
          value.to_string()
        }
      })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn interpolates_variables() {
    let mut context: Context = Context::new();

    context.insert(String::from("user_Id"), json!(String::from("12")));
    context.insert(String::from("Transfer-Encoding"), json!(String::from("chunked")));

    let interpolator = Interpolator::new(&context);
    let url = String::from("http://example.com/users/{{ user_Id }}/view/{{ user_Id }}/{{ Transfer-Encoding }}");
    let interpolated = interpolator.resolve(&url, true);

    assert_eq!(interpolated, "http://example.com/users/12/view/12/chunked");
  }

  #[test]
  #[should_panic]
  fn interpolates_missing_variable() {
    let context: Context = Context::new();

    let interpolator = Interpolator::new(&context);
    let url = String::from("/users/{{ userId }}");
    interpolator.resolve(&url, true);
  }

  #[test]
  fn interpolates_relaxed() {
    let context: Context = Context::new();

    let interpolator = Interpolator::new(&context);
    let url = String::from("/users/{{ userId }}");
    let interpolated = interpolator.resolve(&url, false);

    assert_eq!(interpolated, "/users/");
  }

  #[test]
  fn interpolates_numnamed_variables() {
    let mut context: Context = Context::new();

    context.insert(String::from("zip5"), json!(String::from("90210")));

    let interpolator = Interpolator::new(&context);
    let url = String::from("http://example.com/postalcode/{{ zip5 }}/view/{{ zip5 }}");
    let interpolated = interpolator.resolve(&url, true);

    assert_eq!(interpolated, "http://example.com/postalcode/90210/view/90210");
  }

  #[test]
  fn interpolates_bad_numnamed_variable_names() {
    let mut context: Context = Context::new();

    context.insert(String::from("5digitzip"), json!(String::from("90210")));

    let interpolator = Interpolator::new(&context);
    let url = String::from("http://example.com/postalcode/{{ 5digitzip }}/view/{{ 5digitzip }}");
    let interpolated = interpolator.resolve(&url, true);

    assert_eq!(interpolated, "http://example.com/postalcode/{{ 5digitzip }}/view/{{ 5digitzip }}");
  }

  #[test]
  fn interpolates_environment_variables() {
    std::env::set_var("FOO", "BAR");

    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);
    let url = String::from("http://example.com/postalcode/{{ FOO }}");
    let interpolated = interpolator.resolve(&url, true);

    assert_eq!(interpolated, "http://example.com/postalcode/BAR");
  }
}
