use crate::convert::variables::VariableContext;
use crate::model::Script;
use crate::model::drill::{AssertConfig, AssertItem, AssignConfig, AssignItem, PlanItem, SaveConfig, SaveItem};
use crate::warnings::WarningCollector;
use anyhow::Result;
use regex::Regex;

pub fn convert_script(script: &Script, var_ctx: &VariableContext, warnings: &mut WarningCollector, location: &str) -> Result<Vec<PlanItem>> {
  let lines = get_script_lines(script);
  let mut items = Vec::new();

  for (line_idx, line) in lines.iter().enumerate() {
    let line_loc = format!("{}:line[{}]", location, line_idx);
    let trimmed = line.trim();

    if trimmed.is_empty() || trimmed.starts_with("//") {
      continue;
    }

    let is_response_based = trimmed.contains("pm.response.");

    if is_response_based {
      if let Some(item) = try_convert_test_assertion(trimmed, var_ctx, warnings, &line_loc)? {
        items.push(item);
      } else if let Some(item) = try_convert_test_save(trimmed, var_ctx, warnings, &line_loc)? {
        items.push(item);
      } else if is_unsupported_pattern(trimmed) {
        warnings.warn(&line_loc, format!("Unsupported script pattern: {}", trimmed));
      } else {
        warnings.warn(&line_loc, format!("Cannot convert response-based script line; manual review needed: {}", trimmed));
      }
    } else {
      if let Some(item) = try_convert_pre_request(trimmed, var_ctx, warnings, &line_loc)? {
        items.push(item);
      } else if let Some(item) = try_convert_test_assertion(trimmed, var_ctx, warnings, &line_loc)? {
        items.push(item);
      } else if let Some(item) = try_convert_test_save(trimmed, var_ctx, warnings, &line_loc)? {
        items.push(item);
      } else if is_unsupported_pattern(trimmed) {
        warnings.warn(&line_loc, format!("Unsupported script pattern: {}", trimmed));
      } else {
        warnings.warn(&line_loc, format!("Cannot convert script line; manual review needed: {}", trimmed));
      }
    }
  }

  Ok(items)
}

fn get_script_lines(script: &Script) -> Vec<String> {
  if let Some(src) = &script.src {
    src.lines().map(|s| s.to_string()).collect()
  } else {
    script.exec.clone()
  }
}

fn try_convert_pre_request(line: &str, var_ctx: &VariableContext, warnings: &mut WarningCollector, location: &str) -> Result<Option<PlanItem>> {
  let re = Regex::new(r#"pm\.(environment|collectionVariables|variables|globals)\.set\(["']([^"']+)["']\s*,\s*(.+)\)"#).unwrap();
  if let Some(caps) = re.captures(line) {
    let scope = &caps[1];
    let key = &caps[2];
    let value_expr = &caps[3];

    if scope == "globals" {
      warnings.warn(location, "pm.globals.set not supported; globals not available in Drill");
      return Ok(None);
    }

    let value = interpolate_js_value(value_expr, var_ctx);
    return Ok(Some(PlanItem::Assign(AssignItem {
      name: Some(format!("Set {} variable '{}'", scope, key)),
      assign: AssignConfig {
        key: key.to_string(),
        value,
      },
      weight: None,
    })));
  }

  Ok(None)
}

fn try_convert_test_assertion(line: &str, var_ctx: &VariableContext, _warnings: &mut WarningCollector, _location: &str) -> Result<Option<PlanItem>> {
  // pm.response.to.have.status(200)
  if let Some(caps) = Regex::new(r#"pm\.response\.to\.have\.status\((\d+)\)"#).unwrap().captures(line) {
    return Ok(Some(PlanItem::Assert(AssertItem {
      name: Some("Response status".to_string()),
      assert: AssertConfig::Status {
        _type: "status".to_string(),
        value: serde_yaml::Value::Number(caps[1].parse().unwrap()),
      },
      weight: None,
    })));
  }

  // pm.response.to.have.status([200, 201])
  if let Some(caps) = Regex::new(r#"pm\.response\.to\.have\.status\(\[([^\]]+)\]\)"#).unwrap().captures(line) {
    let codes: Vec<serde_yaml::Value> = caps[1].split(',').map(|s| serde_yaml::Value::Number(s.trim().parse().unwrap())).collect();
    return Ok(Some(PlanItem::Assert(AssertItem {
      name: Some("Response status".to_string()),
      assert: AssertConfig::Status {
        _type: "status".to_string(),
        value: serde_yaml::Value::Sequence(codes),
      },
      weight: None,
    })));
  }

  // pm.response.to.have.header("Content-Type", "application/json")
  if let Some(caps) = Regex::new(r#"pm\.response\.to\.have\.header\(["']([^"']+)["']\s*,\s*["']([^"']+)["']"#).unwrap().captures(line) {
    return Ok(Some(PlanItem::Assert(AssertItem {
      name: Some(format!("Header {}", &caps[1])),
      assert: AssertConfig::Header {
        _type: "header".to_string(),
        key: caps[1].to_string(),
        value: caps[2].to_string(),
      },
      weight: None,
    })));
  }

  // pm.expect(pm.response.code).to.eql(200)
  if let Some(caps) = Regex::new(r#"pm\.expect\(pm\.response\.code\)\.to\.eql\((\d+)\)"#).unwrap().captures(line) {
    return Ok(Some(PlanItem::Assert(AssertItem {
      name: Some("Response status".to_string()),
      assert: AssertConfig::Status {
        _type: "status".to_string(),
        value: serde_yaml::Value::Number(caps[1].parse().unwrap()),
      },
      weight: None,
    })));
  }

  // pm.expect(pm.response.json()...).to.eql(...)
  if let Some(caps) = Regex::new(r#"pm\.expect\(pm\.response\.json\(\)\.([^)]+)\)\.to\.eql\((.+)\)"#).unwrap().captures(line) {
    let path = format!("$.{}", &caps[1]);
    let value = interpolate_js_value(&caps[2], var_ctx);
    return Ok(Some(PlanItem::Assert(AssertItem {
      name: Some(format!("JSON path {}", path)),
      assert: AssertConfig::JsonPath {
        _type: "jsonpath".to_string(),
        key: path,
        value,
      },
      weight: None,
    })));
  }

  // pm.expect(pm.response.jsonPath("$.path")).to.eql("value")
  if let Some(caps) = Regex::new(r#"pm\.expect\(pm\.response\.jsonPath\(["']([^"']+)["']\)\)\.to\.eql\((.+)\)"#).unwrap().captures(line) {
    let value = interpolate_js_value(&caps[2], var_ctx);
    return Ok(Some(PlanItem::Assert(AssertItem {
      name: Some(format!("JSONPath {}", &caps[1])),
      assert: AssertConfig::JsonPath {
        _type: "jsonpath".to_string(),
        key: caps[1].to_string(),
        value,
      },
      weight: None,
    })));
  }

  // pm.expect(pm.response.responseTime).to.be.below(500) / to.be.lessThan(500)
  if let Some(caps) = Regex::new(r#"pm\.expect\(pm\.response\.responseTime\)\.to\.be\.(below|lessThan|above|greaterThan|equal|eq)\((\d+)\)"#).unwrap().captures(line) {
    let operator = match &caps[1] {
      "below" | "lessThan" => "lt",
      "above" | "greaterThan" => "gt",
      "equal" | "eq" => "eq",
      _ => "lt",
    };
    return Ok(Some(PlanItem::Assert(AssertItem {
      name: Some("Response time".to_string()),
      assert: AssertConfig::Duration {
        _type: "duration".to_string(),
        value: caps[2].parse().unwrap(),
        operator: Some(operator.to_string()),
      },
      weight: None,
    })));
  }

  Ok(None)
}

fn try_convert_test_save(line: &str, _var_ctx: &VariableContext, warnings: &mut WarningCollector, location: &str) -> Result<Option<PlanItem>> {
  let re = Regex::new(r#"pm\.(collectionVariables|environment|variables)\.set\(["']([^"']+)["']\s*,\s*(.+)\)"#).unwrap();
  if let Some(caps) = re.captures(line) {
    let scope = &caps[1];
    let key = &caps[2];
    let value_expr = &caps[3].trim();

    if scope == "globals" {
      warnings.warn(location, "pm.globals.set not supported");
      return Ok(None);
    }

    // pm.response.json().path
    if let Some(caps2) = Regex::new(r#"pm\.response\.json\(\)\.([^)\s]+)"#).unwrap().captures(value_expr) {
      let path = format!("$.{}", &caps2[1]);
      return Ok(Some(PlanItem::Save(SaveItem {
        name: Some(format!("Save {} from response", key)),
        save: SaveConfig {
          source: "response_body".to_string(),
          jsonpath: Some(path),
          key: key.to_string(),
        },
        weight: None,
      })));
    }

    // pm.response.jsonPath("$.path")
    if let Some(caps2) = Regex::new(r#"pm\.response\.jsonPath\(["']([^"']+)["']\)"#).unwrap().captures(value_expr) {
      return Ok(Some(PlanItem::Save(SaveItem {
        name: Some(format!("Save {} from response", key)),
        save: SaveConfig {
          source: "response_body".to_string(),
          jsonpath: Some(caps2[1].to_string()),
          key: key.to_string(),
        },
        weight: None,
      })));
    }

    // pm.response.headers.get("Header-Name")
    if let Some(caps2) = Regex::new(r#"pm\.response\.headers\.get\(["']([^"']+)["']\)"#).unwrap().captures(value_expr) {
      return Ok(Some(PlanItem::Save(SaveItem {
        name: Some(format!("Save header {}", &caps2[1])),
        save: SaveConfig {
          source: "response_headers".to_string(),
          jsonpath: None,
          key: caps2[1].to_string(),
        },
        weight: None,
      })));
    }

    // pm.response.status
    if value_expr.contains("pm.response.status") || value_expr.contains("pm.response.code") {
      return Ok(Some(PlanItem::Save(SaveItem {
        name: Some(format!("Save status as {}", key)),
        save: SaveConfig {
          source: "response_status".to_string(),
          jsonpath: None,
          key: key.to_string(),
        },
        weight: None,
      })));
    }

    // pm.response.url
    if value_expr.contains("pm.response.url") {
      return Ok(Some(PlanItem::Save(SaveItem {
        name: Some(format!("Save URL as {}", key)),
        save: SaveConfig {
          source: "response_url".to_string(),
          jsonpath: None,
          key: key.to_string(),
        },
        weight: None,
      })));
    }

    warnings.warn(location, format!("Cannot convert save expression: {}", value_expr));
  }

  Ok(None)
}

fn is_unsupported_pattern(line: &str) -> bool {
  let unsupported = ["pm.sendRequest", "postman.setNextRequest", "pm.cookies", "pm.visualizer", "require(", "module.exports", "console.log", "console.error"];
  unsupported.iter().any(|p| line.contains(p))
}

fn interpolate_js_value(expr: &str, var_ctx: &VariableContext) -> String {
  let mut result = expr.trim().to_string();

  if (result.starts_with('"') && result.ends_with('"')) || (result.starts_with('\'') && result.ends_with('\'')) {
    result = result[1..result.len() - 1].to_string();
  }

  result = var_ctx.interpolate(&result);
  result = crate::convert::request::normalize_interpolations(&result);

  result
}
