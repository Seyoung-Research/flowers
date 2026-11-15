//! Scripting console / API for workspaces, samples, populations, gates,
//! statistics, charts and tables (Phase 9, X1).

use std::cell::RefCell;
use std::rc::Rc;

use thiserror::Error;

use crate::fcs::FcsFile;
use crate::statistics::PopulationProfile;
use crate::workspace::{Population, Sample, Workspace};

/// Errors produced by scripting runs.
#[derive(Debug, Error)]
pub enum ScriptError {
    #[error("script error: {0}")]
    Script(String),
}

/// Context handed to one scripting run.
#[derive(Debug, Clone)]
pub struct ScriptContext {
    pub workspace: Workspace,
}

impl ScriptContext {
    pub fn new(workspace: Workspace) -> Self {
        Self { workspace }
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn sample_ids(&self) -> Vec<String> {
        self.workspace.sample_order.clone()
    }

    pub fn get_sample(&self, id: &str) -> Option<&Sample> {
        self.workspace.samples.get(id)
    }

    pub fn add_sample(&mut self, id: &str, fcs: FcsFile) {
        self.workspace.add_sample(id, fcs);
    }

    pub fn population(&self, sample_id: &str, name: &str) -> Option<Rc<RefCell<Population>>> {
        let sample = self.workspace.get_sample(sample_id)?;
        for population in &sample.root_populations {
            if population.borrow().name == name {
                return Some(Rc::clone(population));
            }
        }
        None
    }

    pub fn statistics(&mut self, sample_id: &str, population: &str) -> Option<PopulationProfile> {
        let root = self.population(sample_id, population)?;
        let indices = root.borrow().event_indices.clone();
        let path = format!("/{}", root.borrow().name);
        self.workspace
            .compute_profile_for_with(sample_id, &path, &indices, &[])
            .into()
    }
}

/// A named scripting handler.
#[derive(Debug, Clone)]
pub struct Script {
    pub name: String,
    pub description: String,
    pub run: fn(context: &mut ScriptContext) -> Result<(), ScriptError>,
}

/// Registry of scripting handlers.
#[derive(Debug, Default)]
pub struct ScriptingEngine {
    scripts: Vec<Script>,
}

impl ScriptingEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, script: Script) -> usize {
        self.scripts.push(script);
        self.scripts.len()
    }

    pub fn names(&self) -> Vec<String> {
        self.scripts
            .iter()
            .map(|script| script.name.clone())
            .collect()
    }

    pub fn run(&mut self, name: &str, context: &mut ScriptContext) -> Result<(), ScriptError> {
        let script = self
            .scripts
            .iter()
            .find(|script| script.name == name)
            .ok_or_else(|| ScriptError::Script(format!("script '{name}' not found")))?;
        (script.run)(context)
    }

    pub fn len(&self) -> usize {
        self.scripts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty()
    }
}

/// Run a named scripting handler against a context.
pub fn run_script(name: &str, context: &mut ScriptContext) -> Result<(), ScriptError> {
    let mut engine = ScriptingEngine::new();
    engine.run(name, context)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> ScriptContext {
        ScriptContext::new(crate::workspace::Workspace::new())
    }

    fn add_sample_script(context: &mut ScriptContext) -> Result<(), ScriptError> {
        let fcs = crate::fcs::FcsFile {
            version: "FCS3.0".to_string(),
            keywords: Default::default(),
            parameters: Vec::new(),
            event_count: 0,
            data: ndarray::Array2::<f32>::zeros((0, 0)),
        };
        context.add_sample("scripted", fcs);
        Ok(())
    }

    #[test]
    fn scripting_engine_runs_registered_script() {
        let mut engine = ScriptingEngine::new();
        engine.register(Script {
            name: "add_sample".to_string(),
            description: "adds a scripted sample".to_string(),
            run: add_sample_script,
        });
        assert_eq!(engine.names(), ["add_sample"]);
        let mut context = context();
        engine.run("add_sample", &mut context).expect("run script");
        assert_eq!(context.sample_ids(), ["scripted"]);
        assert!(context.get_sample("scripted").is_some());
    }

    #[test]
    fn scripting_engine_rejects_unknown_script() {
        let mut engine = ScriptingEngine::new();
        let mut context = context();
        let error = engine
            .run("missing", &mut context)
            .expect_err("unknown script");
        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn script_context_exposes_workspace_and_samples() {
        let mut workspace = crate::workspace::Workspace::new();
        workspace.add_sample(
            "s1",
            crate::fcs::FcsFile {
                version: "FCS3.0".to_string(),
                keywords: Default::default(),
                parameters: Vec::new(),
                event_count: 0,
                data: ndarray::Array2::<f32>::zeros((0, 0)),
            },
        );
        let context = ScriptContext::new(workspace);
        assert_eq!(context.sample_ids(), ["s1"]);
        assert!(context.get_sample("s1").is_some());
        assert!(context.get_sample("nope").is_none());
        assert_eq!(context.workspace().sample_order.len(), 1);
    }

    #[test]
    fn script_context_statistics_returns_none_for_unknown_population() {
        let mut context = context();
        assert!(context.statistics("missing", "Live").is_none());
    }
}
