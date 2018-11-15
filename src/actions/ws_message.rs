use std::collections::HashMap;

use yaml_rust::Yaml;
use colored::*;
use serde_json;
use time;

use ws::{connect, CloseCode};

use interpolator;
use config;

use actions::{Runnable, Report};

#[derive(Clone)]
pub struct WSMessage {
  name: String,
  url: String,
  time: f64,
  data: Option<String>,
  pub with_item: Option<Yaml>,
  pub assign: Option<String>,
}

impl WSMessage {
  pub fn is_that_you(item: &Yaml) -> bool{
    item["websocket_message"].as_hash().is_some()
  }

  pub fn new(item: &Yaml, with_item: Option<Yaml>) -> WSMessage {
    let reference: Option<&str> = item["assign"].as_str();
    let data: Option<&str> = item["websocket_message"]["data"].as_str();

    WSMessage {
      name: item["name"].as_str().unwrap().to_string(),
      url: item["websocket_message"]["url"].as_str().unwrap().to_string(),
      time: 0.0,
      data: data.map(str::to_string),
      with_item: with_item,
      assign: reference.map(str::to_string),
    }
  }

  fn send_request(&self, context: &mut HashMap<String, Yaml>, responses: &mut HashMap<String, serde_json::Value>, _config: &config::Config) -> (f64) {

    let begin = time::precise_time_s();

    let interpolated_name;
    let interpolated_url;

    // Resolve the url
    {
      let interpolator = interpolator::Interpolator::new(context, responses);
      interpolated_name = interpolator.resolve(&self.name);
      interpolated_url = interpolator.resolve(&self.url);
    }

    let data_clone = match &self.data {
        Some(data) => &data,
        None => "",
    };

    if let Err(error) = connect(interpolated_url.clone(), |out| {
        // Queue a message to be sent when the WebSocket is open
        if out.send(data_clone).is_err() {
            println!("Websocket couldn't queue an initial message.")
        } else {
            println!("Client sent message 'Hello WebSocket'. ")
        }

        // The handler needs to take ownership of out, so we use move
        move |msg| {
            // Handle messages received on this connection
            println!("Client got message '{}'. ", msg);

            // Close the connection
            out.close(CloseCode::Normal)
        }
    }) {
        // Inform the user of failure
        println!("Failed to create WebSocket due to: {:?}", error);
    }

    let duration_ms = (time::precise_time_s() - begin) * 1000.0;

    let status_text = if true {
      "ok".to_string().red()
    } else {
      "not ok".to_string().yellow()
    };

    println!("{:width$} {} {} {}{}", interpolated_name.green(), interpolated_url.blue().bold(), status_text, duration_ms.round().to_string().cyan(), "ms".cyan(), width=25);

    duration_ms
  }
}

impl Runnable for WSMessage {
  fn execute(&self, context: &mut HashMap<String, Yaml>, responses: &mut HashMap<String, serde_json::Value>, reports: &mut Vec<Report>, config: &config::Config) {
    if self.with_item.is_some() {
      context.insert("item".to_string(), self.with_item.clone().unwrap());
    }

    let duration_ms = self.send_request(context, responses, config);

    reports.push(Report { name: self.name.to_owned(), duration: duration_ms, status: 1u16 });
  }

}
