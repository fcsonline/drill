use colored::*;
use lazy_static::lazy_static;
use regex::{Captures, Regex};
use serde_json::json;

use crate::benchmark::Context;
use crate::faker;

static INTERPOLATION_PREFIX: &str = "{{";
static INTERPOLATION_SUFFIX: &str = "}}";

lazy_static! {
  pub static ref INTERPOLATION_REGEX: Regex = {
    let regexp = format!("{}{}{}", regex::escape(INTERPOLATION_PREFIX), r" *(\$?[a-zA-Z]+[a-zA-Z\-\._\$0-9\[\]\(\),]*) *", regex::escape(INTERPOLATION_SUFFIX));

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

        if let Some(item) = self.resolve_context_interpolation(capture) {
          return item;
        }

        if let Some(item) = resolve_dynamic(capture) {
          return item;
        }

        if let Some(item) = resolve_faker(capture) {
          return item;
        }

        if let Some(item) = self.resolve_environment_interpolation(capture) {
          return item;
        }

        let message = unknown_variable_message(capture);

        if strict {
          panic!("{}", message);
        }

        eprintln!("{} {}", "WARNING!".yellow().bold(), message);

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

/// Resolve Postman-style dynamic variables (`$guid`, `$randomInt(1,100)`, ...). The `$` prefix is optional.
fn resolve_dynamic(capture: &str) -> Option<String> {
  let name = capture.strip_prefix('$').unwrap_or(capture);

  match name {
    // Identity
    "guid" | "randomUUID" | "uuid" => faker::resolve("uuid"),

    // Time
    "timestamp" => {
      let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
      Some(secs.to_string())
    }
    "isoTimestamp" => Some(chrono::Utc::now().to_rfc3339()),

    // Numbers
    name if name.starts_with("randomInt(") => {
      let (min, max) = parse_range::<i64>(name, "randomInt")?;
      Some(rand::random_range(min..=max).to_string())
    }
    name if name.starts_with("randomFloat(") => {
      let (min, max) = parse_range::<f64>(name, "randomFloat")?;
      Some(rand::random_range(min..max).to_string())
    }
    "randomBoolean" => faker::resolve("boolean"),

    // Strings
    name if name.starts_with("randomHex") => Some(random_hex(parse_optional_len(name, "randomHex"))),
    name if name.starts_with("randomAlphaNumeric") => Some(random_alphanumeric(parse_optional_len(name, "randomAlphaNumeric"))),

    // Fake-backed
    "randomFirstName" => faker::resolve("first_name"),
    "randomLastName" => faker::resolve("last_name"),
    "randomFullName" => faker::resolve("name"),
    "randomEmail" => faker::resolve("email"),
    "randomPhoneNumber" => faker::resolve("phone"),
    "randomCity" => faker::resolve("city"),
    "randomStreetAddress" => faker::resolve("street"),
    "randomCountry" => faker::resolve("country"),
    "randomIp" => faker::resolve("ip"),

    _ => None,
  }
}

fn parse_arg_list<'a>(name: &'a str, prefix: &str) -> Option<Vec<&'a str>> {
  let inner = name.strip_prefix(prefix)?.strip_prefix('(')?.strip_suffix(')')?;
  Some(inner.split(',').collect())
}

fn parse_range<T>(name: &str, prefix: &str) -> Option<(T, T)>
where
  T: std::str::FromStr,
{
  let args = parse_arg_list(name, prefix)?;
  if args.len() != 2 {
    return None;
  }
  Some((args[0].parse().ok()?, args[1].parse().ok()?))
}

fn parse_optional_len(name: &str, prefix: &str) -> usize {
  parse_arg_list(name, prefix).and_then(|args| args.into_iter().next()).and_then(|arg| arg.parse().ok()).unwrap_or(16)
}

fn random_hex(len: usize) -> String {
  const CHARS: &[u8] = b"0123456789abcdef";
  (0..len).map(|_| CHARS[rand::random_range(..CHARS.len())] as char).collect()
}

fn random_alphanumeric(len: usize) -> String {
  const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  (0..len).map(|_| CHARS[rand::random_range(..CHARS.len())] as char).collect()
}

fn resolve_faker(capture: &str) -> Option<String> {
  let prefix = "fake.";

  if let Some(rest) = capture.strip_prefix(prefix) {
    let (locale, key) = parse_fake_locale(rest);

    if locale == "en" {
      return faker::resolve(key);
    }

    return faker::resolve_locale(locale, key);
  }

  None
}

fn parse_fake_locale(rest: &str) -> (&str, &str) {
  let locales = ["zh_cn", "zh_tw", "fr_fr", "de_de", "it_it", "ja_jp", "pt_br", "pt_pt", "ar_sa", "cy_gb"];

  match rest.split_once('.') {
    Some((head, tail)) if locales.contains(&head) => (head, tail),
    _ => ("en", rest),
  }
}

fn unknown_variable_message(variable: &str) -> String {
  // `index` is only seeded inside with_items/range/csv/file expansions. When it
  // is missing the user most likely wants the iteration counter, so point them there.
  if variable == "index" {
    "Unknown 'index' variable! It is only available inside 'with_items', 'with_items_range', 'with_items_from_csv' or 'with_items_from_file' requests. Use 'iteration' for the current iteration number.".to_string()
  } else {
    format!("Unknown '{variable}' variable!")
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

    assert_eq!(interpolator.resolve("{{ Null }}", true), "".to_string());
    assert_eq!(interpolator.resolve("{{ Bool }}", true), "true".to_string());
    assert_eq!(interpolator.resolve("{{ Number }}", true), "12".to_string());
    assert_eq!(interpolator.resolve("{{ String }}", true), "string".to_string());
    assert_eq!(interpolator.resolve("{{ Array }}", true), "[\"a\",\"b\",\"c\"]".to_string());
    assert_eq!(interpolator.resolve("{{ Object }}", true), "{\"this\":\"that\"}".to_string());
    assert_eq!(interpolator.resolve("{{ Nested.this.that.those[2].deee.eeee }}", true), "eeep".to_string());
    assert_eq!(interpolator.resolve("{{ ArrayNested[0].a[1].aaa[0].aaaa }}", true), "123".to_string());
    assert_eq!(interpolator.resolve("{{ ArrayNested[0].a[1].aaa[0].$aaaa }}", true), "$123".to_string());
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
  #[should_panic(expected = "Use 'iteration'")]
  fn index_outside_expansion_suggests_iteration() {
    let context: Context = Context::new();

    let interpolator = Interpolator::new(&context);
    interpolator.resolve("/users/{{ index }}", true);
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
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("FOO", "BAR") };

    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);
    let url = String::from("http://example.com/postalcode/{{ FOO }}");
    let interpolated = interpolator.resolve(&url, true);

    assert_eq!(interpolated, "http://example.com/postalcode/BAR");
  }

  #[test]
  fn interpolates_fake_values() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    let url = interpolator.resolve("/users/{{ fake.name }}", true);
    assert!(!url.contains("{{"));
    assert!(url.starts_with("/users/"));
    assert!(url.len() > "/users/".len());
  }

  #[test]
  fn interpolates_fake_values_in_body() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    let body = interpolator.resolve("{\"email\":\"{{ fake.email }}\"}", true);
    assert!(body.contains('@'));
  }

  #[test]
  fn interpolates_multiple_fake_values() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    let url = interpolator.resolve("/users/{{ fake.first_name }}/{{ fake.last_name }}", true);
    assert!(!url.contains("{{"));
    assert!(url.starts_with("/users/"));
  }

  #[test]
  fn fake_values_can_be_overridden_by_context() {
    let mut context: Context = Context::new();
    context.insert(String::from("fake"), json!({"name": "Override"}));

    let interpolator = Interpolator::new(&context);
    let url = interpolator.resolve("/users/{{ fake.name }}", true);
    assert_eq!(url, "/users/Override");
  }

  #[test]
  fn interpolates_localized_fake_values() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    let url = interpolator.resolve("/users/{{ fake.zh_cn.name }}", true);
    assert!(!url.contains("{{"));
    assert!(url.starts_with("/users/"));
  }

  #[test]
  fn interpolates_localized_fake_values_in_body() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    let body = interpolator.resolve("{\"ville\":\"{{ fake.fr_fr.city }}\"}", true);
    assert!(!body.contains("{{"));
  }

  #[test]
  fn resolves_dynamic_guid_variants() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    for variable in ["$guid", "guid", "$randomUUID", "randomUUID", "$uuid", "uuid"] {
      let value = interpolator.resolve(&format!("{{{{ {variable} }}}}"), true);
      assert_eq!(value.len(), 36, "{variable} should be a hyphenated UUID");
      assert_eq!(value.chars().filter(|&c| c == '-').count(), 4, "{variable} not hyphenated");
      assert_eq!(value.chars().nth(14), Some('4'), "{variable} not a v4 UUID");
    }
  }

  #[test]
  fn resolves_dynamic_timestamps() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    let before = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let ts: u64 = interpolator.resolve("{{ $timestamp }}", true).parse().unwrap();
    let after = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert!((before..=after).contains(&ts));

    let iso = interpolator.resolve("{{ isoTimestamp }}", true);
    assert!(iso.contains('T'));
    assert!(iso.ends_with("+00:00"));
  }

  #[test]
  fn resolves_dynamic_random_int() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    for _ in 0..20 {
      let value: i64 = interpolator.resolve("{{ $randomInt(1,100) }}", true).parse().unwrap();
      assert!((1..=100).contains(&value));
    }

    let value: i64 = interpolator.resolve("{{ randomInt(10,20) }}", true).parse().unwrap();
    assert!((10..=20).contains(&value));
  }

  #[test]
  fn resolves_dynamic_random_float() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    for _ in 0..20 {
      let value: f64 = interpolator.resolve("{{ $randomFloat(1.5,2.5) }}", true).parse().unwrap();
      assert!((1.5..2.5).contains(&value));
    }
  }

  #[test]
  fn resolves_dynamic_random_hex() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    let value = interpolator.resolve("{{ $randomHex }}", true);
    assert_eq!(value.len(), 16);
    assert!(value.chars().all(|c| c.is_ascii_hexdigit()));

    let value = interpolator.resolve("{{ $randomHex(32) }}", true);
    assert_eq!(value.len(), 32);
    assert!(value.chars().all(|c| c.is_ascii_hexdigit()));
  }

  #[test]
  fn resolves_dynamic_random_alphanumeric() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    let value = interpolator.resolve("{{ $randomAlphaNumeric }}", true);
    assert_eq!(value.len(), 16);
    assert!(value.chars().all(|c| c.is_ascii_alphanumeric()));

    let value = interpolator.resolve("{{ $randomAlphaNumeric(24) }}", true);
    assert_eq!(value.len(), 24);
    assert!(value.chars().all(|c| c.is_ascii_alphanumeric()));
  }

  #[test]
  fn resolves_dynamic_random_boolean() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    let value = interpolator.resolve("{{ $randomBoolean }}", true);
    assert!(value == "true" || value == "false");
  }

  #[test]
  fn resolves_dynamic_fake_backed_values() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    assert!(interpolator.resolve("{{ $randomFirstName }}", true).len() > 1);
    assert!(interpolator.resolve("{{ $randomLastName }}", true).len() > 1);
    assert!(interpolator.resolve("{{ $randomFullName }}", true).contains(' '));
    assert!(interpolator.resolve("{{ $randomEmail }}", true).contains('@'));
    assert!(!interpolator.resolve("{{ $randomPhoneNumber }}", true).is_empty());
    assert!(!interpolator.resolve("{{ $randomCity }}", true).is_empty());
    assert!(!interpolator.resolve("{{ $randomStreetAddress }}", true).is_empty());
    assert!(!interpolator.resolve("{{ $randomCountry }}", true).is_empty());
    assert!(!interpolator.resolve("{{ $randomIp }}", true).is_empty());
  }

  #[test]
  fn dynamic_variables_work_in_urls() {
    let context: Context = Context::new();
    let interpolator = Interpolator::new(&context);

    let url = interpolator.resolve("/api?ts={{ $timestamp }}&id={{ $guid }}&name={{ $randomFirstName }}", true);
    assert!(!url.contains("{{"));
    assert!(url.starts_with("/api?ts="));

    let url = interpolator.resolve("/api?range={{ $randomInt(1,100) }}&hex={{ $randomHex(32) }}", true);
    assert!(!url.contains("{{"));
    assert!(url.starts_with("/api?range="));
  }

  #[test]
  fn context_takes_precedence_over_dynamic_variables() {
    let mut context: Context = Context::new();
    context.insert(String::from("$guid"), json!("from-context"));
    context.insert(String::from("$timestamp"), json!("from-context"));

    let interpolator = Interpolator::new(&context);

    assert_eq!(interpolator.resolve("{{ $guid }}", true), "from-context");
    assert_eq!(interpolator.resolve("{{ $timestamp }}", true), "from-context");
  }
}
