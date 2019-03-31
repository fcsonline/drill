use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::Future;
use yaml_rust::Yaml;

use crate::actions::{Report, Runnable};
use crate::config;
use futures::prelude::*;
use futures::stream;

#[derive(Clone)]
pub struct Iteration {
  pub number: i64,
  pub context: Arc<Mutex<HashMap<String, Yaml>>>,
  pub responses: Arc<Mutex<HashMap<String, serde_json::Value>>>,
  pub reports: Arc<Mutex<Vec<Report>>>,
}

impl Iteration {
  pub fn future<'a>(&'a self, benchmark: &'a Arc<Vec<Box<(Runnable + Sync + Send)>>>, config: &'a config::Config) -> Box<Future<Item = (), Error = ()> + Send + 'a> {
    let items: Vec<_> = benchmark.iter().map(move |item| item.execute(&self.context, &self.responses, &self.reports, config)).collect();

    let benchmark_stream = stream::iter_ok::<_, ()>(items);

    // FIXME: try to use flatten
    let work = benchmark_stream.fold(0, move |acc, step| step.map(move |_a| acc + 1)).map(|_a| ()).map_err(|_a| ());

    Box::new(work)
  }
}
