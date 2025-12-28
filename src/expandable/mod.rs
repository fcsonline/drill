pub mod include;

mod multi_csv_request;
mod multi_file_request;
mod multi_iter_request;
mod multi_request;

use serde_yaml::Value;

pub fn pick(item: &Value, with_items: &[Value]) -> usize {
  match item.get("pick").and_then(|v| v.as_i64()) {
    Some(value) => {
      if value.is_negative() {
        panic!("pick option should not be negative, but was {}", value);
      } else if value as usize > with_items.len() {
        panic!("pick option should not be greater than the provided items, but was {}", value);
      } else {
        value as usize
      }
    }
    None => with_items.len(),
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
      let item = &crate::reader::read_file_as_yml_from_str(text)[0];
      let pick = pick(item, item.get("with_items").and_then(|v| v.as_sequence()).unwrap());

      assert_eq!(pick, 2);
    }

    #[test]
    fn should_return_the_with_items_length_if_unset() {
      let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\nwith_items:\n  - 1\n  - 2\n  - 3";
      let item = &crate::reader::read_file_as_yml_from_str(text)[0];
      let pick = pick(item, item.get("with_items").and_then(|v| v.as_sequence()).unwrap());

      assert_eq!(pick, 3);
    }

    #[test]
    #[should_panic(expected = "pick option should not be negative, but was -1")]
    fn should_panic_for_negative_values() {
      let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\npick: -1\nwith_items:\n  - 1\n  - 2\n  - 3";
      let item = &crate::reader::read_file_as_yml_from_str(text)[0];
      pick(item, item.get("with_items").and_then(|v| v.as_sequence()).unwrap());
    }

    #[test]
    #[should_panic(expected = "pick option should not be greater than the provided items, but was 4")]
    fn should_panic_for_values_greater_than_the_items_list() {
      let text = "---\nname: foobar\nrequest:\n  url: /api/{{ item }}\npick: 4\nwith_items:\n  - 1\n  - 2\n  - 3";
      let item = &crate::reader::read_file_as_yml_from_str(text)[0];
      pick(item, item.get("with_items").and_then(|v| v.as_sequence()).unwrap());
    }
  }
}
