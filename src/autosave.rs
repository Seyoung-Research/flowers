//! Autosave, revert-to-saved-state, workspace undo history, batched-save
//! protection and annotation scrapbooks (Phase 9, W9).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::fcs::FcsFile;
use crate::workspace::{Population, PopulationSnapshot, Workspace};

/// Errors produced by autosave, undo, batching and scrapbook operations.
#[derive(Debug, Error)]
pub enum AutosaveError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("snapshot error: {0}")]
    Snapshot(String),
}

/// Autosave configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AutosaveSettings {
    /// Whether autosave is enabled.
    pub enabled: bool,
    /// Save interval in seconds.
    pub interval_seconds: u64,
    /// Timestamp of the last save, as a unix seconds value.
    pub last_saved: Option<u64>,
}

impl AutosaveSettings {
    pub fn new() -> Self {
        Self {
            enabled: true,
            interval_seconds: 30,
            last_saved: None,
        }
    }
}

/// Snapshot of one sample's analysis state (keywords + gated populations).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SampleSnapshot {
    pub id: String,
    pub path: String,
    pub keywords: HashMap<String, String>,
    pub gates: Vec<GateSnapshot>,
}

/// Snapshot of a gated population.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateSnapshot {
    pub name: String,
    pub count: usize,
    pub gate_summary: Option<String>,
}

/// Serializable snapshot of a workspace analysis.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub samples: Vec<SampleSnapshot>,
    pub populations: Vec<PopulationSnapshot>,
}

impl Workspace {
    /// Capture a serializable snapshot of the current analysis.
    pub fn snapshot(&self) -> WorkspaceSnapshot {
        let mut samples = Vec::new();
        for id in &self.sample_order {
            let Some(sample) = self.samples.get(id) else {
                continue;
            };
            samples.push(SampleSnapshot {
                id: id.clone(),
                path: sample
                    .fcs_file
                    .keywords
                    .get("$FIL")
                    .cloned()
                    .unwrap_or_else(|| id.clone()),
                keywords: sample.fcs_file.keywords.clone(),
                gates: sample
                    .gates
                    .iter()
                    .map(|gate| GateSnapshot {
                        name: gate.name.clone(),
                        count: gate.count(),
                        gate_summary: Some(gate.gate_summary.clone()),
                    })
                    .collect(),
            });
        }
        let mut populations = Vec::new();
        for sample in self.samples.values() {
            for population in &sample.root_populations {
                populations.push(Population::to_snapshot(population));
            }
        }
        WorkspaceSnapshot {
            samples,
            populations,
        }
    }
}

/// Rebuild a workspace from a snapshot into the target workspace.
fn restore_into(
    workspace: &mut Workspace,
    snapshot: &WorkspaceSnapshot,
) -> Result<(), AutosaveError> {
    workspace.samples.clear();
    workspace.sample_order.clear();
    for sample in &snapshot.samples {
        let fcs = FcsFile {
            version: "FCS3.0".to_string(),
            keywords: sample.keywords.clone(),
            parameters: Vec::new(),
            event_count: 0,
            data: ndarray::Array2::<f32>::zeros((0, 0)),
        };
        workspace.add_sample(&sample.id, fcs);
    }
    Ok(())
}

/// Revert-to-saved-state holder: keeps the last saved snapshot.
#[derive(Debug, Clone, Default)]
pub struct SavedState {
    pub snapshot: Option<WorkspaceSnapshot>,
}

impl SavedState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn save(&mut self, snapshot: WorkspaceSnapshot) {
        self.snapshot = Some(snapshot);
    }

    pub fn restore(&self, workspace: &mut Workspace) -> Result<(), AutosaveError> {
        let Some(snapshot) = &self.snapshot else {
            return Err(AutosaveError::Snapshot("no saved snapshot".to_string()));
        };
        restore_into(workspace, snapshot)
    }

    pub fn clear(&mut self) {
        self.snapshot = None;
    }
}

/// Snapshot-based undo / redo history for a workspace.
#[derive(Debug, Clone, Default)]
pub struct UndoHistory {
    /// Older states first; the last entry is the current state.
    past: Vec<WorkspaceSnapshot>,
    /// Redo stack.
    future: Vec<WorkspaceSnapshot>,
}

impl UndoHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, snapshot: WorkspaceSnapshot) {
        self.past.push(snapshot);
        self.future.clear();
    }

    pub fn can_undo(&self) -> bool {
        self.past.len() >= 2
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Undo the current state and restore the previous one.
    pub fn undo(&mut self, workspace: &mut Workspace) -> Result<(), AutosaveError> {
        if self.past.len() < 2 {
            return Err(AutosaveError::Snapshot("nothing to undo".to_string()));
        }
        let current = self.past.pop().expect("past has at least two entries");
        self.future.push(current);
        let previous = self
            .past
            .last()
            .expect("past has at least one entry")
            .clone();
        restore_into(workspace, &previous)
    }

    /// Redo a previously undone state.
    pub fn redo(&mut self, workspace: &mut Workspace) -> Result<(), AutosaveError> {
        let Some(snapshot) = self.future.pop() else {
            return Err(AutosaveError::Snapshot("nothing to redo".to_string()));
        };
        self.past.push(snapshot.clone());
        restore_into(workspace, &snapshot)
    }
}

/// Batched-save protection: accumulate dirty flags and commit once.
#[derive(Debug, Clone, Default)]
pub struct BatchedSave {
    dirty: bool,
}

impl BatchedSave {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn commit(&mut self) -> bool {
        let was_dirty = self.dirty;
        self.dirty = false;
        was_dirty
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

/// Annotation scrapbook of experimental figures / pages.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScrapbookEntry {
    pub id: String,
    pub title: String,
    pub svg: String,
}

#[derive(Debug, Clone, Default)]
pub struct Scrapbook {
    entries: Vec<ScrapbookEntry>,
}

impl Scrapbook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, entry: ScrapbookEntry) {
        self.entries.push(entry);
    }

    pub fn get(&self, id: &str) -> Option<&ScrapbookEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        before != self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn entries(&self) -> &[ScrapbookEntry] {
        &self.entries
    }
}

/// Save / load autosave snapshots to a JSON file.
pub fn save_snapshot_file(path: &Path, snapshot: &WorkspaceSnapshot) -> Result<(), AutosaveError> {
    let text = serde_json::to_string_pretty(snapshot)
        .map_err(|error| AutosaveError::Snapshot(error.to_string()))?;
    fs::write(path, text)?;
    Ok(())
}

pub fn load_snapshot_file(path: &Path) -> Result<WorkspaceSnapshot, AutosaveError> {
    let text = fs::read_to_string(path).map_err(AutosaveError::Io)?;
    serde_json::from_str(&text).map_err(|error| AutosaveError::Snapshot(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_fcs() -> FcsFile {
        FcsFile {
            version: "FCS3.0".to_string(),
            keywords: HashMap::new(),
            parameters: Vec::new(),
            event_count: 0,
            data: ndarray::Array2::<f32>::zeros((0, 0)),
        }
    }

    fn workspace_with_id(id: &str) -> Workspace {
        let mut workspace = Workspace::new();
        workspace.add_sample(id, fake_fcs());
        workspace
    }

    #[test]
    fn workspace_snapshot_captures_samples() {
        let workspace = Workspace::new();
        let snapshot = workspace.snapshot();
        assert!(snapshot.samples.is_empty());
        assert!(snapshot.populations.is_empty());

        let mut with_sample = Workspace::new();
        with_sample.add_sample(
            "s1",
            FcsFile {
                version: "FCS3.0".to_string(),
                keywords: HashMap::from([("$CYT".to_string(), "Cytometer".to_string())]),
                parameters: Vec::new(),
                event_count: 0,
                data: ndarray::Array2::<f32>::zeros((0, 0)),
            },
        );
        let snapshot = with_sample.snapshot();
        assert_eq!(snapshot.samples.len(), 1);
        assert_eq!(snapshot.samples[0].id, "s1");
        assert_eq!(
            snapshot.samples[0].keywords.get("$CYT").map(String::as_str),
            Some("Cytometer")
        );
        assert_eq!(snapshot.samples[0].gates.len(), 0);
    }

    #[test]
    fn saved_state_saves_and_restores_snapshot() {
        let workspace = workspace_with_id("s1");
        let snapshot = workspace.snapshot();
        let mut saved = SavedState::new();
        saved.save(snapshot);
        let mut workspace = workspace_with_id("s2");
        saved.restore(&mut workspace).expect("restore snapshot");
        assert!(workspace.samples.contains_key("s1"));
        assert!(!workspace.samples.contains_key("s2"));
    }

    #[test]
    fn undo_history_undoes_and_redoes() {
        let workspace = Workspace::new();
        let mut history = UndoHistory::new();
        assert!(!history.can_undo());
        assert!(!history.can_redo());

        history.push(workspace.snapshot());
        assert!(!history.can_undo());

        let mut workspace = workspace_with_id("s1");
        history.push(workspace.snapshot());
        assert!(history.can_undo());

        history.undo(&mut workspace).expect("undo");
        assert!(!workspace.samples.contains_key("s1"));
        assert!(history.can_redo());

        history.redo(&mut workspace).expect("redo");
        assert!(workspace.samples.contains_key("s1"));
        assert!(!history.can_redo());
        assert!(history.can_undo());
    }

    #[test]
    fn batched_save_flags_dirty_and_commits() {
        let mut batch = BatchedSave::new();
        assert!(!batch.is_dirty());
        batch.mark_dirty();
        assert!(batch.is_dirty());
        assert!(batch.commit());
        assert!(!batch.is_dirty());
    }

    #[test]
    fn scrapbook_add_remove_and_clear() {
        let mut scrapbook = Scrapbook::new();
        assert!(scrapbook.is_empty());
        scrapbook.add(ScrapbookEntry {
            id: "fig1".to_string(),
            title: "Experiment 1".to_string(),
            svg: "<svg/>".to_string(),
        });
        assert_eq!(scrapbook.len(), 1);
        assert!(scrapbook.get("fig1").is_some());
        assert!(scrapbook.get("missing").is_none());
        assert!(scrapbook.remove("fig1"));
        assert!(scrapbook.is_empty());
        assert!(!scrapbook.remove("fig1"));
    }

    #[test]
    fn autosave_snapshot_file_round_trip() {
        let snapshot = WorkspaceSnapshot::default();
        let path = std::env::temp_dir().join(format!(
            "flowers-autosave-{}-snapshot.json",
            std::process::id()
        ));
        save_snapshot_file(&path, &snapshot).expect("save snapshot");
        let restored = load_snapshot_file(&path).expect("load snapshot");
        assert_eq!(restored.samples.len(), snapshot.samples.len());
        assert_eq!(restored.populations.len(), snapshot.populations.len());
        let _ = fs::remove_file(&path);
    }
}
