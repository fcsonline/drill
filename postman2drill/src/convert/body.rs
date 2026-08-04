// Body conversion is handled in request.rs
// This module exists for API completeness
#![allow(dead_code)]

use crate::convert::variables::VariableContext;
use crate::model::Body;
use crate::model::drill::DrillBody;
use crate::warnings::WarningCollector;
use anyhow::Result;

pub fn convert_body(body: Body, var_ctx: &VariableContext, warnings: &mut WarningCollector, location: &str) -> Result<DrillBody> {
  // Implementation in request.rs::convert_body
  super::request::convert_body(body, var_ctx, warnings, location)
}
