//! FlowJo-compatible workspace, template and keyword-notation import/export
//! (Phase 9, W7).
//!
//! - [`FlowJoWorkspaceDocument`] is the FlowJo workspace (WSP) shape: format tag,
//!   version and a flat list of samples plus gated populations.
//! - [`export_flowjo_workspace_json`] / [`import_flowjo_workspace_json`] convert
//!   between a [`crate::workspace::Workspace`] and the FlowJo document.
//! - [`export_jot`] / [`import_jot`] read and write FlowJo keyword notation (JOT)
//!   files.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::archive::ArchiveError;
use crate::fcs::{FcsError, FcsParser};
use crate::workspace::Workspace;

/// Errors produced by FlowJo format import/export.
#[derive(Debug, Error)]
pub enum FlowJoError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("FCS error: {0}")]
    Fcs(#[from] FcsError),
    #[error("archive error: {0}")]
    Archive(#[from] ArchiveError),
}

/// One sample entry of a FlowJo workspace document (WSP).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowJoSample {
    /// Sample name.
    pub name: String,
    /// Source FCS file path.
    pub path: String,
    /// FCS keywords (metadata).
    pub keywords: HashMap<String, String>,
    /// Event count reported by the FCS file.
    pub event_count: usize,
}

/// One gated population entry of a FlowJo workspace document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowJoPopulation {
    /// Sample the population belongs to.
    pub sample: String,
    /// Population / gate name.
    pub name: String,
    /// Event count inside the population.
    pub count: usize,
    /// Optional human-readable gate summary.
    pub gate_summary: Option<String>,
}

/// A FlowJo workspace document (WSP) with samples and gated populations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowJoWorkspaceDocument {
    /// Format tag, for example `flowjo`.
    pub format: String,
    /// Document version.
    pub version: u32,
    /// Samples.
    pub samples: Vec<FlowJoSample>,
    /// Gated populations.
    pub populations: Vec<FlowJoPopulation>,
}

impl FlowJoWorkspaceDocument {
    pub fn new(version: u32) -> Self {
        Self {
            format: "flowjo".to_string(),
            version,
            samples: Vec::new(),
            populations: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// Write the document to a file.
    pub fn write_to(&self, path: &Path) -> Result<(), FlowJoError> {
        let text = self
            .to_json()
            .map_err(|error| FlowJoError::Archive(ArchiveError::Archive(error.to_string())))?;
        fs::write(path, text)?;
        Ok(())
    }

    /// Read the document from a file.
    pub fn read_from(path: &Path) -> Result<Self, FlowJoError> {
        let text = fs::read_to_string(path).map_err(FlowJoError::Io)?;
        Self::from_json(&text)
            .map_err(|error| FlowJoError::Archive(ArchiveError::Archive(error.to_string())))
    }

    /// Rebuild a workspace from the document.
    pub fn to_workspace(&self) -> Result<Workspace, FlowJoError> {
        let mut workspace = Workspace::new();
        for sample in &self.samples {
            let bytes = fs::read(Path::new(&sample.path)).map_err(FlowJoError::Io)?;
            let fcs = FcsParser::parse_bytes(&bytes).map_err(FlowJoError::Fcs)?;
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

/// Export a workspace into a FlowJo-compatible workspace document (WSP).
pub fn export_flowjo_workspace_json(
    workspace: &Workspace,
) -> Result<FlowJoWorkspaceDocument, FlowJoError> {
    let mut document = FlowJoWorkspaceDocument::new(1);
    for id in &workspace.sample_order {
        let Some(sample) = workspace.samples.get(id) else {
            continue;
        };
        document.samples.push(FlowJoSample {
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
            document.populations.push(FlowJoPopulation {
                sample: id.clone(),
                name: gate.name.clone(),
                count: gate.count(),
                gate_summary: Some(gate.gate_summary.clone()),
            });
        }
    }
    Ok(document)
}

/// Import a FlowJo-compatible workspace document back into a workspace.
pub fn import_flowjo_workspace_json(text: &str) -> Result<Workspace, FlowJoError> {
    let document = FlowJoWorkspaceDocument::from_json(text)
        .map_err(|error| FlowJoError::Archive(ArchiveError::Archive(error.to_string())))?;
    document.to_workspace()
}

/// Export keyword notation (JOT) text from a keyword map.
pub fn export_jot(keywords: &HashMap<String, String>, path: &Path) -> Result<(), FlowJoError> {
    let mut text = String::new();
    for (key, value) in keywords {
        text.push_str(&format!("{key}/{value}\n"));
    }
    fs::write(path, text)?;
    Ok(())
}

/// Import keyword notation (JOT) text back into a keyword map.
pub fn import_jot(text: &str) -> HashMap<String, String> {
    let mut keywords = HashMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('/') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        keywords.insert(key.to_string(), value.trim().to_string());
    }
    keywords
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::WorkspaceArchive;
    use crate::fcs::Parameter;
    use crate::merge::FcsBuilder;
    use crate::workspace::Workspace;

    fn workspace_with_sample() -> Workspace {
        let mut workspace = Workspace::new();
        let fcs_path = std::env::temp_dir().join(format!(
            "flowers-flowjo-{}-sample_a.fcs",
            std::process::id()
        ));
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
        workspace.add_sample("sample_a", fcs);
        workspace
    }

    #[test]
    fn flowjo_workspace_document_round_trips_through_json() {
        let document = FlowJoWorkspaceDocument::new(1);
        let json = document.to_json().expect("serialize flowjo document");
        assert!(json.contains("\"format\": \"flowjo\""));
        let restored =
            FlowJoWorkspaceDocument::from_json(&json).expect("deserialize flowjo document");
        assert_eq!(restored.format, "flowjo");
        assert_eq!(restored.version, 1);
        assert!(restored.samples.is_empty());
    }

    #[test]
    fn flowjo_workspace_export_and_import_round_trip() {
        let workspace = workspace_with_sample();
        let document = export_flowjo_workspace_json(&workspace).expect("export flowjo");
        assert_eq!(document.format, "flowjo");
        assert_eq!(document.version, 1);
        assert_eq!(document.samples.len(), 1);
        assert_eq!(document.samples[0].name, "sample_a");
        assert_eq!(
            document.samples[0].keywords.get("$CYT").map(String::as_str),
            Some("Cytometer")
        );
        assert_eq!(document.samples[0].event_count, 3);

        let text = document.to_json().expect("serialize document");
        let restored = import_flowjo_workspace_json(&text).expect("import flowjo document");
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

    #[test]
    fn flowjo_workspace_write_and_read_file_round_trip() {
        let workspace = workspace_with_sample();
        let document = export_flowjo_workspace_json(&workspace).expect("export flowjo");
        let path = std::env::temp_dir().join(format!(
            "flowers-flowjo-{}-workspace.json",
            std::process::id()
        ));
        document.write_to(&path).expect("write flowjo document");
        let restored = FlowJoWorkspaceDocument::read_from(&path).expect("read flowjo document");
        assert_eq!(restored, document);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn jot_export_and_import_round_trip() {
        let mut keywords = HashMap::new();
        keywords.insert("$CYT".to_string(), "Cytometer".to_string());
        keywords.insert("$FIL".to_string(), "sample_a.fcs".to_string());
        let path =
            std::env::temp_dir().join(format!("flowers-flowjo-{}-sample.jot", std::process::id()));
        export_jot(&keywords, &path).expect("write jot");
        let text = fs::read_to_string(&path).expect("read jot");
        let restored = import_jot(&text);
        assert_eq!(restored.get("$CYT").map(String::as_str), Some("Cytometer"));
        assert_eq!(
            restored.get("$FIL").map(String::as_str),
            Some("sample_a.fcs")
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn flowjo_archive_round_trips_through_json() {
        let archive = WorkspaceArchive::new("experiment", crate::project::Project::default());
        let json = archive.to_json().expect("serialize archive");
        let restored = WorkspaceArchive::from_json(&json).expect("deserialize archive");
        assert_eq!(restored.name, "experiment");
    }
}
