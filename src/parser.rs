use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BenchmarkConfig {
    base: String,
    iterations: Option<i64>,
    concurrency: Option<i64>,
    rampup: Option<i64>,
    plan: Vec<Action>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Action {
    name: String,
    request: Option<Request>,
    assign: Option<String>,
    shuffle: Option<bool>,
    with_items: Option<Vec<Item>>,
    assert: Option<Assert>,
    with_items_range: Option<WithItemsRange>,
    with_items_from_csv: Option<String>,
    pick: Option<i64>,
    exec: Option<Exec>,
    tags: Option<Vec<String>>,
    delay: Option<Delay>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Request {
    method: Option<String>,
    url: String,
    headers: Option<HashMap<String, String>>,
    body: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Assert {
    key: String,
    value: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WithItemsRange {
    start: i64,
    step: i64,
    stop: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Exec {
    command: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Delay {
    milliseconds: Option<i64>,
    seconds: Option<i64>,
    minutes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum Item {
    Num(usize),
    Map(HashMap<String, MapValue>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum MapValue {
    Num(usize),
    String(String),
    Bool(bool),
}

impl From<MapValue> for String {
    fn from(v: MapValue) -> Self {
        match v {
            MapValue::Num(n) => n.to_string(),
            MapValue::String(s) => s,
            MapValue::Bool(b) => b.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::parser::{BenchmarkConfig, Item, MapValue};

    #[test]
    fn happy_path() {
        let yaml_str = "
            base: 'http://localhost:9000'
            iterations: 10000

            plan:
                - name: Fetch users
                  request:
                    url: /api/users.json

                - name: Fetch some users by hash
                  assign: fetchuser
                  request:
                  url: /api/users/{{ item.id }}
                  shuffle: true
                  with_items:
                    - { id: 70 }
                    - { id: 73 }
                    - { id: 75 }
        ";

        let benchmark_config: BenchmarkConfig = match serde_yaml_ng::from_str(yaml_str) {
            Ok(config) => config,
            Err(err) => panic!("Failed to parse YAML: {}", err),
        };

        assert_eq!(benchmark_config.base, "http://localhost:9000");
    }

    #[test]
    fn deserialize_with_items_nums() {
        let yaml_str = "
            base: 'http://localhost:9000'
            iterations: 10000

            plan:
                - name: With items list
                  with_items:
                    - 70
                    - 73
                    - 75
        ";

        let benchmark_config: BenchmarkConfig = match serde_yaml_ng::from_str(yaml_str) {
            Ok(config) => config,
            Err(err) => panic!("Failed to parse YAML: {}", err),
        };
        assert!(benchmark_config.plan[0].with_items.is_some(), "Array of numbers not parsed");

        let nums: Vec<usize> = benchmark_config.plan[0]
            .with_items
            .as_ref()
            .unwrap()
            .iter()
            .map(|elem| match elem {
                Item::Num(num) => *num,
                _ => panic!("Expected number"),
            })
            .collect();
        assert_eq!(nums.as_slice().len(), 3, "Array of numbers not fully parsed");

        let expect = [70, 73, 75];

        for item in nums.into_iter().enumerate() {
            assert_eq!(expect[item.0], item.1, "Item element does not match");
        }
    }

    #[test]
    fn deserialize_with_items_maps() {
        let yaml_str = "
            base: 'http://localhost:9000'
            iterations: 10000

            plan:
                - name: With items map
                  with_items:
                    - { id: 70 }
                    - { id: 73 }
                    - { id: 75 }
        ";

        let benchmark_config: BenchmarkConfig = match serde_yaml_ng::from_str(yaml_str) {
            Ok(config) => config,
            Err(err) => panic!("Failed to parse YAML: {}", err),
        };
        assert!(benchmark_config.plan[0].with_items.is_some(), "Array of maps not parsed");

        let nums: Vec<HashMap<String, MapValue>> = benchmark_config.plan[0]
            .with_items
            .as_ref()
            .unwrap()
            .iter()
            .map(|elem| match elem {
                Item::Map(map) => map.clone(),
                _ => panic!("Expected map"),
            })
            .collect();
        assert_eq!(nums.as_slice().len(), 3, "Array of maps not fully parsed");

        let expect = [HashMap::from([("id".to_string(), MapValue::Num(70))]), HashMap::from([("id".to_string(), MapValue::Num(73))]), HashMap::from([("id".to_string(), MapValue::Num(75))])];

        for item in nums.into_iter().enumerate() {
            let to_compare = item.1;
            let to_expect = expect.get(item.0).unwrap();
            assert_eq!(to_expect.len(), to_compare.len(), "Map size does not match");

            assert_eq!(
                match to_expect["id"] {
                    MapValue::Num(id) => id,
                    _ => panic!("Expected number"),
                },
                match to_compare["id"] {
                    MapValue::Num(id) => id,
                    _ => panic!("Expected number"),
                },
                "Item element does not match"
            );
        }
    }
}
