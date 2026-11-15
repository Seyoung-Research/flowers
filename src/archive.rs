//! Workspace templates (WSPT), ACS archives, Excel workspace export and
//! workspace merging (Phase 9, W6).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::fcs::{FcsError, FcsParser};
use crate::project::Project;
use crate::workspace::{Population, Workspace};

/// Errors produced by workspace templates, archives, exports and merges.
#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("FCS error: {0}")]
    Fcs(#[from] FcsError),
    #[error("template error: {0}")]
    Template(String),
    #[error("archive error: {0}")]
    Archive(String),
}

/// One sample described by a workspace template (WSPT).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateSample {
    /// Sample id used when the template is instantiated.
    pub id: String,
    /// Optional source path (absolute, or relative to the template root).
    pub source_path: Option<String>,
    /// Optional inline FCS bytes; when absent the file is read from `source_path`.
    pub fcs_bytes: Option<Vec<u8>>,
    /// FCS keywords (metadata) applied to the instantiated sample.
    pub keywords: HashMap<String, String>,
}

/// A workspace template: a named collection of sample definitions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceTemplate {
    pub name: String,
    pub description: String,
    pub version: u32,
    pub samples: Vec<TemplateSample>,
}

impl WorkspaceTemplate {
    pub fn new(name: &str, description: &str, version: u32) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            version,
            samples: Vec::new(),
        }
    }

    pub fn with_sample(mut self, sample: TemplateSample) -> Self {
        self.samples.push(sample);
        self
    }
}

/// Registry of workspace templates (WSPT).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceTemplateRegistry {
    pub templates: Vec<WorkspaceTemplate>,
}

impl WorkspaceTemplateRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, template: WorkspaceTemplate) -> usize {
        let name = template.name.to_ascii_lowercase();
        if let Some(existing) = self
            .templates
            .iter_mut()
            .find(|t| t.name.to_ascii_lowercase() == name)
        {
            *existing = template;
            return self.templates.len();
        }
        self.templates.push(template);
        self.templates.len()
    }

    pub fn get(&self, name: &str) -> Option<&WorkspaceTemplate> {
        self.templates
            .iter()
            .find(|template| template.name.eq_ignore_ascii_case(name))
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        let before = self.templates.len();
        self.templates
            .retain(|template| template.name.to_ascii_lowercase() != name);
        before != self.templates.len()
    }

    pub fn names(&self) -> Vec<&str> {
        self.templates
            .iter()
            .map(|template| template.name.as_str())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    /// Instantiate a template as a fresh workspace.
    ///
    /// Each template sample becomes a workspace sample; inline FCS bytes win over
    /// the on-disk source path.
    pub fn instantiate(&self, name: &str, root: &Path) -> Result<Workspace, ArchiveError> {
        let template = self.get(name).ok_or_else(|| {
            ArchiveError::Template(format!("workspace template '{name}' not found"))
        })?;
        let mut workspace = Workspace::new();
        for sample in &template.samples {
            let fcs = if let Some(bytes) = &sample.fcs_bytes {
                FcsParser::parse_bytes(bytes)?
            } else {
                let path = root.join(sample.source_path.as_deref().unwrap_or(""));
                let bytes = fs::read(&path)?;
                FcsParser::parse_bytes(&bytes)?
            };
            let mut fcs = fcs;
            for (key, value) in &sample.keywords {
                fcs.keywords.insert(key.clone(), value.clone());
            }
            workspace.add_sample(&sample.id, fcs);
        }
        Ok(workspace)
    }
}

/// A serializable workspace archive: a named snapshot of a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceArchive {
    pub name: String,
    pub version: u32,
    pub project: Project,
}

impl WorkspaceArchive {
    pub fn new(name: &str, project: Project) -> Self {
        Self {
            name: name.to_string(),
            version: crate::project::PROJECT_VERSION,
            project,
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub fn save(&self, path: &Path) -> Result<(), ArchiveError> {
        let text = self
            .to_json()
            .map_err(|error| ArchiveError::Archive(error.to_string()))?;
        fs::write(path, text)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, ArchiveError> {
        let text = fs::read_to_string(path).map_err(ArchiveError::Io)?;
        Self::from_json(&text).map_err(|error| ArchiveError::Archive(error.to_string()))
    }
}

/// Export a workspace to an Excel workbook (XLSX) as bytes.
///
/// Sheet `Samples` carries one row per sample; sheet `Populations` carries one
/// row per gated population with its count and gate summary.
pub fn workspace_to_xlsx(workspace: &Workspace) -> Result<Vec<u8>, ArchiveError> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let samples_sheet = workbook.add_worksheet();
    samples_sheet
        .set_name("Samples")
        .map_err(|error| ArchiveError::Archive(error.to_string()))?;
    samples_sheet
        .write_row(0, 0, ["Sample", "Source Path", "Events", "Gates"])
        .map_err(|error| ArchiveError::Archive(error.to_string()))?;
    for (index, id) in workspace.sample_order.iter().enumerate() {
        let sample = workspace.samples.get(id);
        let events = sample
            .map(|sample| sample.fcs_file.event_count)
            .unwrap_or(0);
        let gates = sample.map(|sample| sample.gates.len()).unwrap_or(0);
        samples_sheet
            .write_row(
                index as u32 + 1,
                0,
                [
                    id.as_str(),
                    sample
                        .and_then(|sample| sample.fcs_file.keywords.get("$FIL"))
                        .map(String::as_str)
                        .unwrap_or(""),
                    &events.to_string(),
                    &gates.to_string(),
                ],
            )
            .map_err(|error| ArchiveError::Archive(error.to_string()))?;
    }
    let populations_sheet = workbook.add_worksheet();
    populations_sheet
        .set_name("Populations")
        .map_err(|error| ArchiveError::Archive(error.to_string()))?;
    populations_sheet
        .write_row(
            0,
            0,
            ["Sample", "Population", "Count", "Total", "GateSummary"],
        )
        .map_err(|error| ArchiveError::Archive(error.to_string()))?;
    for id in &workspace.sample_order {
        let rows = crate::merge::population_rows_for_sample(workspace, id);
        for (index, row) in rows.iter().enumerate() {
            populations_sheet
                .write_row(
                    index as u32 + 2,
                    0,
                    [
                        row.sample.as_str(),
                        row.population.as_str(),
                        &row.count.to_string(),
                        &row.total.to_string(),
                        row.gate_summary.as_deref().unwrap_or(""),
                    ],
                )
                .map_err(|error| ArchiveError::Archive(error.to_string()))?;
        }
    }
    workbook
        .save_to_buffer()
        .map_err(|error| ArchiveError::Archive(error.to_string()))
}

/// Write a workspace to an Excel workbook (XLSX) file.
pub fn write_workspace_xlsx(path: &Path, workspace: &Workspace) -> Result<(), ArchiveError> {
    let bytes = workspace_to_xlsx(workspace)?;
    fs::write(path, bytes)?;
    Ok(())
}

/// Merge two workspaces into one (W6 workspace merging).
///
/// Samples with equal ids from `right` override the left-hand side; new samples
/// from either side are kept. Population trees of the same sample are merged by
/// population name (event indices are unioned and children merged).
pub fn merge_workspaces(left: &Workspace, right: &Workspace) -> Workspace {
    let mut merged = left.clone();
    for id in &right.sample_order {
        let Some(source) = merged.samples.get_mut(id) else {
            let sample = right.samples.get(id).expect("sample in order");
            merged.samples.insert(id.clone(), sample.clone());
            merged.sample_order.push(id.clone());
            continue;
        };
        // Union population trees by name.
        for population in &right.samples[id].root_populations {
            let target: Option<Rc<RefCell<Population>>> = source
                .root_populations
                .iter()
                .position(|candidate| candidate.borrow().name == population.borrow().name)
                .map(|index| Rc::clone(&source.root_populations[index]));
            if let Some(mut target) = target {
                merge_population_trees(&mut target, population);
            }
        }
        source.gates = merge_gates(&source.gates, &right.samples[id].gates);
    }
    merged
}

fn merge_population_trees(target: &mut Rc<RefCell<Population>>, source: &Rc<RefCell<Population>>) {
    let source_borrow = source.borrow();
    if source_borrow.name == target.borrow().name {
        let mut target_borrow = target.borrow_mut();
        target_borrow
            .event_indices
            .extend(source_borrow.event_indices.iter().copied());
        target_borrow.event_indices.sort_unstable();
        target_borrow.event_indices.dedup();
        for child in &source_borrow.children {
            let child_target: Option<Rc<RefCell<Population>>> = target_borrow
                .children
                .iter()
                .find(|candidate| candidate.borrow().name == child.borrow().name)
                .map(Rc::clone);
            if let Some(mut child_target) = child_target {
                merge_population_trees(&mut child_target, child);
            }
        }
    }
}

fn merge_gates(
    left: &[crate::gating::GatedPopulation],
    right: &[crate::gating::GatedPopulation],
) -> Vec<crate::gating::GatedPopulation> {
    let mut merged = left.to_vec();
    for gate in right {
        if let Some(existing) = merged
            .iter_mut()
            .find(|candidate| candidate.name == gate.name)
        {
            existing
                .event_indices
                .extend(gate.event_indices.iter().copied());
            existing.event_indices.sort_unstable();
            existing.event_indices.dedup();
            existing.gate_summary = gate.gate_summary.clone();
        } else {
            merged.push(gate.clone());
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcs::Parameter;
    use crate::merge::{FcsBuilder, fcs_to_bytes};
    use crate::workspace::Workspace;

    fn template_with_sample() -> WorkspaceTemplate {
        WorkspaceTemplate::new("My Experiment", "control + treated", 1).with_sample(
            TemplateSample {
                id: "S1".to_string(),
                source_path: Some("/data/S1.fcs".to_string()),
                fcs_bytes: None,
                keywords: HashMap::from([("$CYT".to_string(), "Cytometer".to_string())]),
            },
        )
    }

    fn registry_with(template: WorkspaceTemplate) -> WorkspaceTemplateRegistry {
        let mut registry = WorkspaceTemplateRegistry::new();
        registry.add(template);
        registry
    }

    #[test]
    fn template_registry_add_get_remove_and_names() {
        let mut registry = registry_with(template_with_sample());
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.names(), ["My Experiment"]);
        assert!(registry.get("my experiment").is_some());
        assert!(registry.get("missing").is_none());
        assert!(registry.remove("My Experiment"));
        assert!(registry.is_empty());
        assert!(!registry.remove("My Experiment"));
    }

    #[test]
    fn template_registry_round_trips_through_json() {
        let registry = registry_with(template_with_sample());
        let json = serde_json::to_string(&registry).expect("serialize registry");
        let restored: WorkspaceTemplateRegistry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, registry);
        assert_eq!(restored.names(), ["My Experiment"]);
    }

    #[test]
    fn template_instantiate_reads_fcs_from_root() {
        let mut builder = FcsBuilder::new("FCS3.0");
        builder.add_keyword("$CYT", "Cytometer");
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
        let bytes = fcs_to_bytes(&fcs).expect("serialize fcs");

        let root =
            std::env::temp_dir().join(format!("flowers-archive-{}-template", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create template root");
        let fcs_path = root.join("S1.fcs");
        fs::write(&fcs_path, &bytes).expect("write fcs");

        let registry = WorkspaceTemplateRegistry {
            templates: vec![WorkspaceTemplate {
                name: "file based".to_string(),
                description: "from disk".to_string(),
                version: 2,
                samples: vec![TemplateSample {
                    id: "S1".to_string(),
                    source_path: Some("S1.fcs".to_string()),
                    fcs_bytes: None,
                    keywords: HashMap::new(),
                }],
            }],
        };
        let workspace = registry
            .instantiate("file based", &root)
            .expect("instantiate template");
        assert_eq!(workspace.sample_order, ["S1"]);
        assert_eq!(
            workspace.samples["S1"]
                .fcs_file
                .keywords
                .get("$CYT")
                .map(String::as_str),
            Some("Cytometer")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn workspace_archive_round_trips_through_json_and_file() {
        let project = crate::project::Project::default();
        let archive = WorkspaceArchive::new("experiment", project);
        let json = archive.to_json().expect("serialize archive");
        let restored = WorkspaceArchive::from_json(&json).expect("deserialize archive");
        assert_eq!(restored.name, "experiment");
        assert_eq!(restored.version, restored.version);
        let path = std::env::temp_dir().join(format!(
            "flowers-archive-{}-archive.wsp",
            std::process::id()
        ));
        archive.save(&path).expect("save archive");
        let loaded = WorkspaceArchive::load(&path).expect("load archive");
        assert_eq!(loaded.name, "experiment");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn workspace_xlsx_export_is_a_zip_workbook() {
        let workspace = Workspace::new();
        let bytes = workspace_to_xlsx(&workspace).expect("xlsx export");
        assert!(bytes.starts_with(b"PK"));
        let path = std::env::temp_dir().join(format!(
            "flowers-archive-{}-workspace.xlsx",
            std::process::id()
        ));
        write_workspace_xlsx(&path, &workspace).expect("write xlsx");
        assert!(fs::metadata(&path).expect("xlsx metadata").len() > 0);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn workspace_merging_unions_samples_and_populations() {
        let fcs = FcsBuilder::new("FCS3.0").build().expect("build fcs");
        let mut left = Workspace::new();
        left.add_sample("s1", fcs.clone());
        left.add_root_population("s1", "Live", vec![0, 1])
            .expect("root population");
        let mut right = Workspace::new();
        right.add_sample("s2", fcs.clone());
        right
            .add_root_population("s2", "Live", vec![2, 3])
            .expect("root population");

        let merged = merge_workspaces(&left, &right);
        assert_eq!(merged.sample_order.len(), 2);
        assert_eq!(merged.samples.len(), 2);
        assert_eq!(merged.samples["s1"].root_populations.len(), 1);
        assert_eq!(merged.samples["s2"].root_populations.len(), 1);
    }
}
