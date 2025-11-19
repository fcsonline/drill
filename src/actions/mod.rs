use async_trait::async_trait;
use yaml_rust2::Yaml;

mod assert;
mod assign;
mod delay;
mod exec;
mod request;

pub use self::assert::Assert;
pub use self::assign::Assign;
pub use self::delay::Delay;
pub use self::exec::Exec;
pub use self::request::Request;

use crate::{
    benchmark::{Context, Pool, Reports},
    cli::Args,
};

use std::{fmt, io};

#[async_trait]
pub trait Runnable {
    async fn execute(&self, context: &mut Context, reports: &mut Reports, pool: &Pool, app_args: &Args) -> Result<(), io::Error>;
}

#[derive(Clone)]
pub struct Report {
    pub name: String,
    pub duration: f64,
    pub status: u16,
}

impl fmt::Debug for Report {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "\n- name: {}\n  duration: {}\n", self.name, self.duration)
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "\n- name: {}\n  duration: {}\n  status: {}\n", self.name, self.duration, self.status)
    }
}

pub fn extract_optional<'a>(item: &'a Yaml, attr: &'a str) -> Result<Option<String>, io::Error> {
    if let Some(s) = item[attr].as_str() {
        Ok(Some(s.to_string()))
    } else if item[attr].as_hash().is_some() {
        Err(io::Error::new(io::ErrorKind::InvalidInput, format!("`{}` needs to be a string. Try adding quotes", attr)))
    } else {
        Ok(None)
    }
}

pub fn extract<'a>(item: &'a Yaml, attr: &'a str) -> Result<String, io::Error> {
    match &item[attr] {
        Yaml::String(s) => Ok(s.to_string()),
        Yaml::Integer(i) => Ok(i.to_string()),
        Yaml::Hash(_) => Err(io::Error::new(io::ErrorKind::InvalidInput, format!("`{}` is required needs to be a string. Try adding quotes", attr))),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, format!("Unknown node `{}` => {:?}", attr, item[attr]))),
    }
}
