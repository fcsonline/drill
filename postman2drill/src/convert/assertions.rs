// Additional assertion helpers - mostly covered in scripts.rs
// This module can be extended for more complex assertion patterns
#![allow(dead_code)]

use crate::model::drill::{AssertItem, AssertConfig};

pub fn create_status_assert(name: Option<String>, codes: Vec<u16>) -> AssertItem {
    let value = if codes.len() == 1 {
        serde_yaml::Value::Number(codes[0].into())
    } else {
        serde_yaml::Value::Sequence(codes.into_iter().map(|c| serde_yaml::Value::Number(c.into())).collect())
    };
    AssertItem {
        name,
        assert: AssertConfig::Status { _type: "status".to_string(), value },
        weight: None,
    }
}

pub fn create_header_assert(name: Option<String>, key: String, value: String) -> AssertItem {
    AssertItem {
        name,
        assert: AssertConfig::Header { _type: "header".to_string(), key, value },
        weight: None,
    }
}

pub fn create_jsonpath_assert(name: Option<String>, path: String, value: String) -> AssertItem {
    AssertItem {
        name,
        assert: AssertConfig::JsonPath { _type: "jsonpath".to_string(), key: path, value },
        weight: None,
    }
}

pub fn create_duration_assert(name: Option<String>, max_ms: u64, operator: Option<String>) -> AssertItem {
    AssertItem {
        name,
        assert: AssertConfig::Duration { 
            _type: "duration".to_string(), 
            value: max_ms, 
            operator: operator.or(Some("lt".to_string())),
        },
        weight: None,
    }
}