use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct VariableContext {
    pub vars: HashMap<String, serde_yaml::Value>,
}

impl VariableContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_collection_var(&mut self, key: &str, value: &Value) {
        if let Ok(yaml_val) = serde_yaml::to_value(value) {
            self.vars.insert(key.to_string(), yaml_val);
        }
    }

    pub fn add_env_var(&mut self, key: &str, value: &str) {
        self.vars.insert(key.to_string(), serde_yaml::Value::String(value.to_string()));
    }

    #[allow(dead_code)]
    pub fn get(&self, key: &str) -> Option<&serde_yaml::Value> {
        self.vars.get(key)
    }

    pub fn interpolate(&self, input: &str) -> String {
        let mut result = input.to_string();
        for (key, value) in &self.vars {
            let placeholder = format!("{{{{{}}}}}", key);
            let replacement = match value {
                serde_yaml::Value::String(s) => s.clone(),
                serde_yaml::Value::Number(n) => n.to_string(),
                serde_yaml::Value::Bool(b) => b.to_string(),
                _ => serde_yaml::to_string(value).unwrap_or_default(),
            };
            result = result.replace(&placeholder, &replacement);
        }
        result
    }
}