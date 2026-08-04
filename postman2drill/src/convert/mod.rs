pub mod request;
pub mod body;
pub mod auth;
pub mod scripts;
pub mod variables;
pub mod assertions;

use crate::model::{Collection, Environment, DrillBenchmark, PlanItem, Auth as ModelAuth};
use crate::model::collection::{RequestItem as CollectionRequestItem, Item as CollectionItem};
use crate::model::drill::{RequestItem as DrillRequestItem};
use crate::warnings::WarningCollector;
use crate::convert::variables::VariableContext;
use anyhow::Result;

pub struct Converter {
    warnings: WarningCollector,
    variable_ctx: VariableContext,
    collection_auth: Option<ModelAuth>,
}

impl Converter {
    pub fn new() -> Self {
        Self {
            warnings: WarningCollector::new(),
            variable_ctx: VariableContext::new(),
            collection_auth: None,
        }
    }

    pub fn convert(&mut self, collection: Collection, environment: Option<Environment>) -> Result<DrillBenchmark> {
        for var in &collection.variable {
            if var.disabled != Some(true) {
                self.variable_ctx.add_collection_var(&var.key, &var.value);
            }
        }

        if let Some(env) = environment {
            for val in &env.values {
                if val.enabled {
                    self.variable_ctx.add_env_var(&val.key, &val.value);
                }
            }
        }

        self.collection_auth = collection.auth.clone();

        let mut lifecycle = None;
        if !collection.event.is_empty() {
            let mut setup_items = Vec::new();
            let mut iteration_start_items = Vec::new();

            for event in &collection.event {
                if event.disabled == Some(true) { continue; }
                match event.listen.as_str() {
                    "prerequest" => {
                        let items = self.convert_script(&event.script, "collection.event.prerequest")?;
                        setup_items.extend(items);
                    }
                    "test" => {
                        let items = self.convert_script(&event.script, "collection.event.test")?;
                        iteration_start_items.extend(items);
                    }
                    _ => {}
                }
            }

            if !setup_items.is_empty() || !iteration_start_items.is_empty() {
                lifecycle = Some(crate::model::Lifecycle {
                    setup: if setup_items.is_empty() { None } else { Some(setup_items) },
                    iteration_start: if iteration_start_items.is_empty() { None } else { Some(iteration_start_items) },
                    teardown: None,
                    iteration_stop: None,
                });
            }
        }

        let mut plan = Vec::new();
        for (idx, item) in collection.item.into_iter().enumerate() {
            let items = self.convert_item(item, &format!("item[{}]", idx))?;
            plan.extend(items);
        }

        Ok(DrillBenchmark {
            base: None,
            concurrency: None,
            iterations: None,
            rampup: None,
            vars: if self.variable_ctx.vars.is_empty() { None } else { Some(self.variable_ctx.vars.clone()) },
            lifecycle,
            results: None,
            load_shape: None,
            plan,
        })
    }

    pub fn convert_item(&mut self, item: CollectionItem, location: &str) -> Result<Vec<PlanItem>> {
        match item {
            CollectionItem::Request(req) => {
                self.convert_request(req, location)
            }
            CollectionItem::Folder(folder) => {
                let mut items = Vec::new();
                let parent_auth = self.collection_auth.clone();
                if folder.auth.is_some() {
                    self.collection_auth = folder.auth.clone();
                }
                for (idx, child) in folder.item.into_iter().enumerate() {
                    let child_items = self.convert_item(child, &format!("{}.item[{}]", location, idx))?;
                    items.extend(child_items);
                }
                self.collection_auth = parent_auth;
                Ok(items)
            }
        }
    }

    pub fn convert_request(&mut self, req: CollectionRequestItem, location: &str) -> Result<Vec<PlanItem>> {
        let name = req.name.clone();
        let request = req.request;
        let events = req.event;
        
        let mut items = Vec::new();

        // Pre-request scripts
        for (idx, event) in events.iter().enumerate() {
            if event.disabled == Some(true) { continue; }
            if event.listen == "prerequest" {
                let script_items = self.convert_script(&event.script, &format!("{}.event[{}]", location, idx))?;
                items.extend(script_items);
            }
        }

        // Main request
        let request_item = self.build_request(name, request, location)?;
        items.push(PlanItem::Request(request_item));

        // Test scripts
        for (idx, event) in events.iter().enumerate() {
            if event.disabled == Some(true) { continue; }
            if event.listen == "test" {
                let script_items = self.convert_script(&event.script, &format!("{}.event[{}]", location, idx))?;
                items.extend(script_items);
            }
        }

        Ok(items)
    }

    fn build_request(&mut self, name: String, request: crate::model::Request, location: &str) -> Result<DrillRequestItem> {
        let drill_request = crate::convert::request::convert_request(
            request,
            &self.variable_ctx,
            self.collection_auth.as_ref(),
            &mut self.warnings,
            location,
        )?;

        Ok(DrillRequestItem {
            name: Some(name),
            request: drill_request,
            assign: None,
            weight: None,
            tags: None,
        })
    }

    fn convert_script(&mut self, script: &crate::model::Script, location: &str) -> Result<Vec<PlanItem>> {
        crate::convert::scripts::convert_script(script, &self.variable_ctx, &mut self.warnings, location)
    }

    pub fn into_warnings(self) -> WarningCollector {
        self.warnings
    }
}