//! BD FACSDiva experiment import/export (Phase 9, W8).
//!
//! - [`DivaExperiment`] models a FACSDiva experiment: a name, a version and the
//!   samples (source FCS paths, keyword metadata, event counts) plus the gated
//!   populations reported by the experiment.
//! - [`export_diva_experiment_json`] / [`import_diva_experiment_json`] convert
//!   between a [`crate::workspace::Workspace`] and the Diva experiment document.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::archive::ArchiveError;
use crate::fcs::{FcsError, FcsParser};
use crate::flowjo::FlowJoPopulation;
use crate::workspace::Workspace;

/// Errors produced by BD FACSDiva experiment import/export.
#[derive(Debug, Error)]
pub enum DivaError {
    #[error("IO error: {0}")]
    Io(std::io::Error),
    #[error("FCS error: {0}")]
    Fcs(#[from] FcsError),
    #[error("archive error: {0}")]
    Archive(#[from] ArchiveError),
}

/// One sample of a BD FACSDiva experiment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DivaSample {
    /// Sample (file) name.
    pub name: String,
    /// Source FCS file path.
    pub path: String,
    /// FCS keywords (metadata).
    pub keywords: HashMap<String, String>,
    /// Event count reported by the FCS file.
    pub event_count: usize,
}

/// A BD FACSDiva experiment: samples plus gated populations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DivaExperiment {
    /// Experiment name.
    pub name: String,
    /// Experiment format version.
    pub version: u32,
    /// Samples.
    pub samples: Vec<DivaSample>,
    /// Gated populations.
    pub populations: Vec<FlowJoPopulation>,
}

impl DivaExperiment {
    pub fn new(name: &str, version: u32) -> Self {
        Self {
            name: name.to_string(),
            version,
            samples: Vec::new(),
            populations: Vec::new(),
        }
    }

    pub fn with_sample(mut self, sample: DivaSample) -> Self {
        self.samples.push(sample);
        self
    }

    /// Rebuild a workspace from the experiment document.
    pub fn to_workspace(&self) -> Result<Workspace, DivaError> {
        let mut workspace = Workspace::new();
        for sample in &self.samples {
            let bytes = fs::read(Path::new(&sample.path)).map_err(DivaError::Io)?;
            let fcs = FcsParser::parse_bytes(&bytes).map_err(DivaError::Fcs)?;
            let mut fcs = fcs;
            for (key, value) in &sample.keywords {
                fcs.keywords.insert(key.clone(), value.clone());
            }
            workspace.add_sample(&sample.name, fcs);
        }
        for population in &self.populations {
            let Some(sample) = workspace.samples.get_mut(&population.sample) else {
                continue;
            };
            sample.gates.push(crate::gating::GatedPopulation {
                name: population.name.clone(),
                event_indices: Vec::new(),
                gate_summary: population.gate_summary.clone().unwrap_or_default(),
            });
        }
        Ok(workspace)
    }
}

/// Export a workspace into a BD FACSDiva experiment document.
pub fn export_diva_experiment_json(workspace: &Workspace) -> Result<DivaExperiment, DivaError> {
    let mut experiment = DivaExperiment::new("FACSDiva experiment", 1);
    for id in &workspace.sample_order {
        let Some(sample) = workspace.samples.get(id) else {
            continue;
        };
        experiment.samples.push(DivaSample {
            name: id.clone(),
            path: sample
                .fcs_file
                .keywords
                .get("$FIL")
                .cloned()
                .unwrap_or_else(|| id.clone()),
            keywords: sample.fcs_file.keywords.clone(),
            event_count: sample.fcs_file.event_count,
        });
    }
    for id in &workspace.sample_order {
        let Some(sample) = workspace.samples.get(id) else {
            continue;
        };
        for gate in &sample.gates {
            experiment.populations.push(FlowJoPopulation {
                sample: id.clone(),
                name: gate.name.clone(),
                count: gate.count(),
                gate_summary: Some(gate.gate_summary.clone()),
            });
        }
    }
    Ok(experiment)
}

/// Import a BD FACSDiva experiment document back into a workspace.
pub fn import_diva_experiment_json(text: &str) -> Result<Workspace, DivaError> {
    let experiment: DivaExperiment = serde_json::from_str(text)
        .map_err(|error| DivaError::Archive(ArchiveError::Archive(error.to_string())))?;
    experiment.to_workspace()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcs::Parameter;
    use crate::merge::FcsBuilder;
    use crate::workspace::Workspace;

    fn workspace_with_sample(tag: &str) -> Workspace {
        let fcs_path = std::env::temp_dir().join(format!("flowers-diva-{}-sample_a.fcs", tag));
        let fcs_path_str = fcs_path.to_string_lossy().to_string();
        let mut builder = FcsBuilder::new("FCS3.0");
        builder.add_keyword("$CYT", "Cytometer");
        builder.add_keyword("$FIL", fcs_path_str.as_str());
        builder.add_parameter(Parameter {
            name: "FL1".to_string(),
            description: "FITC".to_string(),
            range: 1024.0,
            exponent: 0.0,
            min: 0.0,
            max: 1024.0,
        });
        builder.add_events([1.0_f32, 2.0, 3.0]);
        let fcs = builder.build().expect("build fcs");
        let _ = fs::write(
            &fcs_path,
            crate::merge::fcs_to_bytes(&fcs).expect("serialize fcs"),
        );
        let mut workspace = Workspace::new();
        workspace.add_sample("sample_a", fcs);
        workspace
    }

    #[test]
    fn diva_experiment_round_trips_through_json() {
        let workspace = workspace_with_sample("import");
        let experiment = export_diva_experiment_json(&workspace).expect("export diva");
        assert_eq!(experiment.name, "FACSDiva experiment");
        assert_eq!(experiment.version, 1);
        assert_eq!(experiment.samples.len(), 1);
        assert_eq!(experiment.samples[0].name, "sample_a");
        assert_eq!(
            experiment.samples[0]
                .keywords
                .get("$CYT")
                .map(String::as_str),
            Some("Cytometer")
        );
        assert_eq!(experiment.samples[0].event_count, 3);
        assert!(experiment.populations.is_empty());

        let text = serde_json::to_string(&experiment).expect("serialize diva");
        let restored: DivaExperiment = serde_json::from_str(&text).expect("deserialize diva");
        assert_eq!(restored, experiment);
    }

    #[test]
    fn diva_experiment_import_rebuilds_workspace() {
        let workspace = workspace_with_sample("round");
        let experiment = export_diva_experiment_json(&workspace).expect("export diva");
        let text = serde_json::to_string(&experiment).expect("serialize diva");
        let restored = import_diva_experiment_json(&text).expect("import diva");
        assert_eq!(restored.sample_order.len(), 1);
        assert_eq!(
            restored
                .samples
                .get("sample_a")
                .expect("sample a")
                .fcs_file
                .keywords
                .get("$CYT")
                .map(String::as_str),
            Some("Cytometer")
        );
    }
}
