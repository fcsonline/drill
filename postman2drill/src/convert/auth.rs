// Auth conversion is handled in request.rs
// This module exists for API completeness
#![allow(dead_code)]

use crate::model::Auth;
use crate::model::drill::DrillAuth;
use crate::convert::variables::VariableContext;
use crate::warnings::WarningCollector;
use anyhow::Result;

pub fn convert_auth(
    auth: Option<&Auth>,
    var_ctx: &VariableContext,
    warnings: &mut WarningCollector,
    location: &str,
) -> Result<Option<DrillAuth>> {
    // Implementation in request.rs::convert_auth
    super::request::convert_auth(auth, var_ctx, warnings, location)
}