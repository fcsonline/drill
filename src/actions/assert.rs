use std::io;

use async_trait::async_trait;
use colored::*;
use serde_json::json;
use yaml_rust2::Yaml;

use crate::actions::extract;
use crate::actions::Runnable;
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;
use crate::interpolator;

#[derive(Clone)]
pub struct Assert {
    name: String,
    key: String,
    value: String,
}

impl Assert {
    pub fn is_that_you(item: &Yaml) -> bool {
        item["assert"].as_hash().is_some()
    }

    pub fn new(item: &Yaml, _with_item: Option<Yaml>) -> Result<Assert, io::Error> {
        let name = extract(item, "name")?;
        let key = extract(&item["assert"], "key")?;
        let value = extract(&item["assert"], "value")?;

        Ok(Assert {
            name,
            key,
            value,
        })
    }
}

#[async_trait]
impl Runnable for Assert {
    async fn execute(&self, context: &mut Context, _reports: &mut Reports, _pool: &Pool, config: &Config) -> Result<(), io::Error> {
        if !config.quiet {
            println!("{:width$} {}={}?", self.name.green(), self.key.cyan().bold(), self.value.magenta(), width = 25);
        }

        let interpolator = interpolator::Interpolator::new(context);
        let eval = format!("{{{{ {} }}}}", &self.key);
        let stored = interpolator.resolve(&eval, true);
        let assertion = json!(self.value.to_owned());

        if !stored.eq(&assertion) {
            Err(io::Error::new(io::ErrorKind::Other, format!("Assertion mismatched: {} != {}", stored, assertion)))
        } else {
            Ok(())
        }
    }
}
