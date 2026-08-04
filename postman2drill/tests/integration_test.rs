use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

fn project_root() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run_converter(args: &[&str]) -> (String, String) {
  let bin = env!("CARGO_BIN_EXE_postman2drill");
  let output = Command::new(bin).args(args).output().expect("failed to execute postman2drill");
  (String::from_utf8_lossy(&output.stdout).to_string(), String::from_utf8_lossy(&output.stderr).to_string())
}

fn temp_output(name: &str) -> PathBuf {
  let path = std::env::temp_dir().join(name);
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  path
}

#[test]
fn test_converter_outputs_expected_yaml() {
  let root = project_root();
  let collection = root.join("tests/fixtures/sample_collection.json");
  let out = temp_output("postman2drill_test_output.yml");

  let (_stdout, stderr) = run_converter(&[collection.to_str().unwrap(), "-o", out.to_str().unwrap()]);

  assert!(stderr.contains("Drill benchmark written to"), "expected success message, got stderr: {}", stderr);

  let yaml = fs::read_to_string(&out).expect("output YAML missing");
  assert!(yaml.contains("base_url: https://api.example.com"));
  assert!(yaml.contains("name: Login"));
  assert!(yaml.contains("method: POST"));
  assert!(yaml.contains("body:"));
  assert!(yaml.contains("save:\n"));
  assert!(yaml.contains("jsonpath: $.token"));
  assert!(yaml.contains("urlencoded:"));
  assert!(yaml.contains("formdata:"));
}

#[test]
fn test_converter_warnings_json() {
  let root = project_root();
  let collection = root.join("tests/fixtures/warnings_collection.json");
  let out = temp_output("postman2drill_test_output2.yml");
  let warnings = temp_output("postman2drill_test_warnings.json");

  let (_stdout, _stderr) = run_converter(&[collection.to_str().unwrap(), "-o", out.to_str().unwrap(), "-w", warnings.to_str().unwrap(), "-f", "json"]);

  let json = fs::read_to_string(&warnings).expect("warnings file missing");
  let parsed: serde_json::Value = serde_json::from_str(&json).expect("warnings not valid JSON");
  assert!(parsed.is_array());
}

#[test]
fn test_converter_environment_override() {
  let root = project_root();
  let collection = root.join("tests/fixtures/sample_collection.json");
  let environment = root.join("tests/fixtures/sample_environment.json");
  let out = temp_output("postman2drill_test_output_env.yml");

  let (_stdout, stderr) = run_converter(&[collection.to_str().unwrap(), environment.to_str().unwrap(), "-o", out.to_str().unwrap()]);

  assert!(stderr.contains("Drill benchmark written to"), "expected success message, got stderr: {}", stderr);

  let yaml = fs::read_to_string(&out).expect("output YAML missing");
  assert!(yaml.contains("base_url: https://env.example.com"));
  assert!(yaml.contains("api_key: env-api-key"));
}

#[test]
fn test_converter_graphql_body() {
  let root = project_root();
  let collection = root.join("tests/fixtures/graphql_collection.json");
  let out = temp_output("postman2drill_test_output_graphql.yml");

  let (_stdout, stderr) = run_converter(&[collection.to_str().unwrap(), "-o", out.to_str().unwrap()]);

  assert!(stderr.contains("Drill benchmark written to"), "expected success message, got stderr: {}", stderr);

  let yaml = fs::read_to_string(&out).expect("output YAML missing");
  assert!(yaml.contains("graphql:"));
  assert!(yaml.contains("query: query GetUsers"));
  assert!(yaml.contains("variables:"));
  assert!(yaml.contains("limit: '10'"));
}

#[test]
fn test_converter_oauth2_auth() {
  let root = project_root();
  let collection = root.join("tests/fixtures/oauth_collection.json");
  let out = temp_output("postman2drill_test_output_oauth.yml");

  let (_stdout, _stderr) = run_converter(&[collection.to_str().unwrap(), "-o", out.to_str().unwrap()]);

  let yaml = fs::read_to_string(&out).expect("output YAML missing");
  assert!(yaml.contains("type: oauth2"));
  assert!(yaml.contains("flow: client_credentials"));
  assert!(yaml.contains("token_url: https://auth.example.com/token"));
  assert!(yaml.contains("client_id: client123"));
  assert!(yaml.contains("client_secret: secret123"));
  assert!(yaml.contains("scope: read write"));
  assert!(yaml.contains("save_token_as: access_token"));
}

#[test]
fn test_converter_folder_auth_inheritance() {
  let root = project_root();
  let collection = root.join("tests/fixtures/folder_collection.json");
  let out = temp_output("postman2drill_test_output_folder.yml");

  let (_stdout, stderr) = run_converter(&[collection.to_str().unwrap(), "-o", out.to_str().unwrap()]);

  assert!(stderr.contains("Drill benchmark written to"), "expected success message, got stderr: {}", stderr);

  let yaml = fs::read_to_string(&out).expect("output YAML missing");
  assert!(yaml.contains("type: bearer"));
  assert!(yaml.contains("token: folder-token"));
  assert!(yaml.contains("url: https://api.example.com/users/{{ user_id }}"));
}

#[test]
fn test_converter_config_yaml() {
  let root = project_root();
  let collection = root.join("tests/fixtures/sample_collection.json");
  let out = temp_output("postman2drill_test_output_config.yml");
  let config = temp_output("postman2drill_test_config.yml");

  {
    let mut f = fs::File::create(&config).expect("config file create failed");
    f.write_all(b"concurrency: 10\niterations: 100\nrampup: 5\n").unwrap();
  }

  let (_stdout, stderr) = run_converter(&[collection.to_str().unwrap(), "-o", out.to_str().unwrap(), "--config", config.to_str().unwrap()]);

  assert!(stderr.contains("Drill benchmark written to"), "expected success message, got stderr: {}", stderr);

  let yaml = fs::read_to_string(&out).expect("output YAML missing");
  assert!(yaml.contains("concurrency: 10"));
  assert!(yaml.contains("iterations: 100"));
  assert!(yaml.contains("rampup: 5"));
}

#[test]
fn test_converter_vars_yaml() {
  let root = project_root();
  let collection = root.join("tests/fixtures/sample_collection.json");
  let out = temp_output("postman2drill_test_output_vars.yml");
  let vars = temp_output("postman2drill_test_vars.yml");

  {
    let mut f = fs::File::create(&vars).expect("vars file create failed");
    f.write_all(b"username: alice\npassword: secret123\n").unwrap();
  }

  let (_stdout, stderr) = run_converter(&[collection.to_str().unwrap(), "-o", out.to_str().unwrap(), "--vars", vars.to_str().unwrap()]);

  assert!(stderr.contains("Drill benchmark written to"), "expected success message, got stderr: {}", stderr);

  let yaml = fs::read_to_string(&out).expect("output YAML missing");
  assert!(yaml.contains("username: alice"));
  assert!(yaml.contains("password: secret123"));
  assert!(yaml.contains("\"username\":\"alice\""));
  assert!(yaml.contains("\"password\":\"secret123\""));
}
