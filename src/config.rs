use yaml_rust::YamlLoader;

use reader;

static NTHREADS: i64 = 1;
static NITERATIONS: i64 = 1;

pub struct Config {
  pub base: String,
  pub threads: i64,
  pub iterations: i64,
  pub no_check_certificate: bool,
}

impl Config {
  pub fn new(path: &str, no_check_certificate: bool) -> Config {
    let config_file = reader::read_file(path);

    let config_docs = YamlLoader::load_from_str(config_file.as_str()).unwrap();
    let config_doc = &config_docs[0];

    let threads = match config_doc["threads"].as_i64() {
      Some(value) => value,
      None => {
        println!("Invalid threads value!");

        NTHREADS
      },
    };

    let iterations = match config_doc["iterations"].as_i64() {
      Some(value) => value,
      None => {
        println!("Invalid iterations value!");

        NITERATIONS
      },
    };

    let base = config_doc["base"].as_str().unwrap().to_owned();

    Config{
      base: base,
      threads: threads,
      iterations: iterations,
      no_check_certificate: no_check_certificate,
    }
  }
}
