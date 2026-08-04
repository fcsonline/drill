use anyhow::Result;
use clap::Parser;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

mod convert;
mod model;
mod warnings;

use convert::Converter;
use model::{Collection, DrillConfigInput};

#[derive(Parser)]
#[command(name = "postman2drill", version, about = "Convert Postman Collection v2.1 to Drill benchmark YAML")]
struct Cli {
  /// Postman collection JSON file
  collection: PathBuf,

  /// Optional Postman environment JSON file
  environment: Option<PathBuf>,

  /// Output Drill YAML file (default: stdout)
  #[arg(short, long)]
  output: Option<PathBuf>,

  /// Drill benchmark config YAML file
  #[arg(short, long)]
  config: Option<PathBuf>,

  /// Variables YAML file
  #[arg(long)]
  vars: Option<PathBuf>,

  /// Warnings report file (default: stderr)
  #[arg(short, long)]
  warnings: Option<PathBuf>,

  /// Warnings format: json or text
  #[arg(short, long, default_value = "json")]
  format: String,

  /// Treat warnings as errors (exit non-zero)
  #[arg(long)]
  strict: bool,
}

fn main() -> Result<()> {
  let cli = Cli::parse();

  let collection_json = fs::read_to_string(&cli.collection)?;
  let collection: Collection = serde_json::from_str(&collection_json)?;

  let environment = if let Some(env_path) = &cli.environment {
    let env_json = fs::read_to_string(env_path)?;
    Some(serde_json::from_str(&env_json)?)
  } else {
    None
  };

  let config_input = if let Some(config_path) = &cli.config {
    let config_yaml = fs::read_to_string(config_path)?;
    Some(serde_yaml::from_str::<DrillConfigInput>(&config_yaml)?)
  } else {
    None
  };

  let vars_file = if let Some(vars_path) = &cli.vars {
    let vars_yaml = fs::read_to_string(vars_path)?;
    Some(serde_yaml::from_str::<HashMap<String, serde_yaml::Value>>(&vars_yaml)?)
  } else {
    None
  };

  let mut converter = Converter::new();
  let drill = converter.convert(collection, environment, config_input, vars_file)?;
  let warnings = converter.into_warnings();

  // Output Drill YAML
  let yaml = serde_yaml::to_string(&drill)?;
  if let Some(out_path) = &cli.output {
    fs::write(out_path, &yaml)?;
    eprintln!("Drill benchmark written to {}", out_path.display());
  } else {
    println!("{}", yaml);
  }

  // Output warnings
  if warnings.has_warnings() || warnings.has_errors() {
    let warning_output = match cli.format.as_str() {
      "json" => warnings.to_json(),
      "text" => warnings.to_text(),
      _ => warnings.to_json(),
    };

    if let Some(warn_path) = &cli.warnings {
      fs::write(warn_path, &warning_output)?;
      eprintln!("Warnings written to {}", warn_path.display());
    } else {
      eprintln!("\n--- Warnings ---");
      eprintln!("{}", warning_output);
    }
  }

  // Exit code
  if cli.strict && (warnings.has_warnings() || warnings.has_errors()) {
    std::process::exit(1);
  }

  if warnings.has_errors() {
    std::process::exit(1);
  }

  Ok(())
}
