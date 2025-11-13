use std::{borrow::Cow, io};

use colored::*;
use lazy_static::lazy_static;
use regex::{Captures, Regex};
use serde_json::json;

use crate::benchmark::Context;

static INTERPOLATION_PREFIX: &str = "{{";
static INTERPOLATION_SUFFIX: &str = "}}";

lazy_static! {
    pub(crate) static ref INTERPOLATION_REGEX: Result<Regex, io::Error> = {
        let regexp = format!("{}{}{}", regex::escape(INTERPOLATION_PREFIX), r" *([a-zA-Z]+[a-zA-Z\-\._\$0-9\[\]]*) *", regex::escape(INTERPOLATION_SUFFIX));

        Regex::new(regexp.as_str()).map_err(|err| io::Error::new(io::ErrorKind::Other, format!("Regex error: {}", err)))
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

    pub fn resolve(&self, url: &str, strict: bool) -> Result<String, io::Error> {
        let le = match INTERPOLATION_REGEX.as_ref() {
            Ok(regex) => regex,
            Err(err) => return Err(io::Error::new(io::ErrorKind::Other, format!("Regex error: {}", err))),
        };

        let mut error: Option<io::Error> = None;

        let result = le.replace_all(url, |caps: &Captures| {
            if error.is_some() {
                return String::new();
            }

            let capture = &caps[1];

            if let Some(item) = self.resolve_context_interpolation(capture) {
                return item;
            }

            if let Some(item) = self.resolve_environment_interpolation(capture) {
                return item;
            }

            if strict {
                error = Some(io::Error::new(io::ErrorKind::InvalidData, format!("Unknown '{}' variable!", capture)));
                return String::new();
            }

            eprintln!("{} Unknown '{}' variable!", "WARNING!".yellow().bold(), &capture);

            String::new()
        });

        if let Some(err) = error {
            return Err(err);
        }

        match result {
            Cow::Borrowed(s) => {
                if strict {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid variable name in {}", s)));
                }

                eprintln!("{} Unknown variable in {}!", "WARNING!".yellow().bold(), s);
                return Ok(String::new());
            }
            owned => Ok(owned.to_string()),
        }
    }

    fn resolve_environment_interpolation(&self, value: &str) -> Option<String> {
        match std::env::vars().find(|tuple| tuple.0 == value) {
            Some(tuple) => Some(tuple.1),
            _ => None,
        }
    }

    fn resolve_context_interpolation(&self, value: &str) -> Option<String> {
        // convert "." and "[" to "/" and "]" to "" to look like a json pointer
        let val: String = format!("/{}", value.replace(['.', '['], "/").replace(']', ""));

        // force the context into a Value, and acess by pointer
        if let Some(item) = json!(self.context).pointer(&val).to_owned() {
            return Some(match item.to_owned() {
                serde_json::Value::Null => "".to_owned(),
                serde_json::Value::Bool(v) => v.to_string(),
                serde_json::Value::Number(v) => v.to_string(),
                serde_json::Value::String(v) => v,
                serde_json::Value::Array(v) => serde_json::to_string(&v).unwrap(),
                serde_json::Value::Object(v) => serde_json::to_string(&v).unwrap(),
            });
        }
        None
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
        assert!(interpolated.is_ok());

        assert_eq!(interpolated.unwrap(), "http://example.com/users/12/view/12/chunked");
    }

    #[test]
    fn interpolates_variables_nested() {
        let mut context: Context = Context::new();

        context.insert(String::from("Null"), serde_json::Value::Null);
        context.insert(String::from("Bool"), json!(true));
        context.insert(String::from("Number"), json!(12));
        context.insert(String::from("String"), json!("string"));
        context.insert(String::from("Array"), json!(["a", "b", "c"]));
        context.insert(String::from("Object"), json!({"this": "that"}));
        context.insert(String::from("Nested"), json!({"this": {"that": {"those": [{"wow": 1}, {"so": 2}, {"deee": {"eeee": "eeep"}}]}}}));
        context.insert(String::from("ArrayNested"), json!([{"a": [{}, {"aa": 2, "aaa": [{"aaaa": 123, "$aaaa": "$123"}]}]}]));

        let interpolator = Interpolator::new(&context);

        let bool = interpolator.resolve("{{ Bool }}", true);
        assert_eq!(bool.is_ok(), true);
        assert_eq!(bool.unwrap(), "true".to_string());

        let number = interpolator.resolve("{{ Number }}", true);
        assert_eq!(number.is_ok(), true);
        assert_eq!(number.unwrap(), "12".to_string());

        let string = interpolator.resolve("{{ String }}", true);
        assert_eq!(string.is_ok(), true);
        assert_eq!(string.unwrap(), "string".to_string());

        let array = interpolator.resolve("{{ Array }}", true);
        assert_eq!(array.is_ok(), true);
        assert_eq!(array.unwrap(), "[\"a\",\"b\",\"c\"]".to_string());

        let object = interpolator.resolve("{{ Object }}", true);
        assert_eq!(object.is_ok(), true);
        assert_eq!(object.unwrap(), "{\"this\":\"that\"}".to_string());

        let nested = interpolator.resolve("{{ Nested.this.that.those[2].deee.eeee }}", true);
        assert_eq!(nested.is_ok(), true);
        assert_eq!(nested.unwrap(), "eeep".to_string());

        let array_nested = interpolator.resolve("{{ ArrayNested[0].a[1].aaa[0].aaaa }}", true);
        assert_eq!(array_nested.is_ok(), true);
        assert_eq!(array_nested.unwrap(), "123".to_string());

        let array_nested_dollar = interpolator.resolve("{{ ArrayNested[0].a[1].aaa[0].$aaaa }}", true);
        assert_eq!(array_nested_dollar.is_ok(), true);
        assert_eq!(array_nested_dollar.unwrap(), "$123".to_string());
    }

    #[test]
    fn interpolates_missing_variable() {
        let context: Context = Context::new();

        let interpolator = Interpolator::new(&context);
        let url = String::from("/users/{{ userId }}");
        let result = interpolator.resolve(&url, true);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn interpolates_relaxed() {
        let context: Context = Context::new();

        let interpolator = Interpolator::new(&context);
        let url = String::from("/users/{{ userId }}");
        let interpolated = interpolator.resolve(&url, false);
        assert_eq!(interpolated.is_ok(), true);

        assert_eq!(interpolated.unwrap(), "/users/");
    }

    #[test]
    fn interpolates_numnamed_variables() {
        let mut context: Context = Context::new();

        context.insert(String::from("zip5"), json!(String::from("90210")));

        let interpolator = Interpolator::new(&context);
        let url = String::from("http://example.com/postalcode/{{ zip5 }}/view/{{ zip5 }}");
        let interpolated = interpolator.resolve(&url, true);
        assert_eq!(interpolated.is_ok(), true);

        assert_eq!(interpolated.unwrap(), "http://example.com/postalcode/90210/view/90210");
    }

    #[test]
    fn interpolates_bad_numnamed_variable_names() {
        let mut context: Context = Context::new();

        context.insert(String::from("5digitzip"), json!(String::from("90210")));

        let interpolator = Interpolator::new(&context);
        let url = String::from("http://example.com/postalcode/{{ 5digitzip }}/view/{{ 5digitzip }}");
        let interpolated = interpolator.resolve(&url, true);
        assert!(interpolated.is_err());

        assert_eq!(interpolated.unwrap_err().to_string(), "Invalid variable name in http://example.com/postalcode/{{ 5digitzip }}/view/{{ 5digitzip }}");
    }

    #[test]
    fn interpolates_environment_variables() {
        std::env::set_var("FOO", "BAR");

        let context: Context = Context::new();
        let interpolator = Interpolator::new(&context);
        let url = String::from("http://example.com/postalcode/{{ FOO }}");
        let interpolated = interpolator.resolve(&url, true);
        assert_eq!(interpolated.is_ok(), true);

        assert_eq!(interpolated.unwrap(), "http://example.com/postalcode/BAR");
    }
}
