use clap::Parser;
use std::fs;
use std::path::PathBuf;
use anyhow::Result;

mod model;
mod convert;
mod warnings;

use model::Collection;
use convert::Converter;

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
    
    // Read collection
    let collection_json = fs::read_to_string(&cli.collection)?;
    let collection: Collection = serde_json::from_str(&collection_json)?;
    
    // Read environment if provided
    let environment = if let Some(env_path) = &cli.environment {
        let env_json = fs::read_to_string(env_path)?;
        Some(serde_json::from_str(&env_json)?)
    } else {
        None
    };
    
    // Convert
    let mut converter = Converter::new();
    let drill = converter.convert(collection, environment)?;
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