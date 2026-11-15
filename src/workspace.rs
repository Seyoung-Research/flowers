use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::fcs::FcsFile;
use crate::gating::GatedPopulation;
use crate::groups::SampleGroups;
use crate::statistics::PopulationProfile;

use serde::{Deserialize, Serialize};

/// A node in the hierarchical population tree.
///
/// Each population holds the event indices (into a sample's data matrix) that
/// pass its gate, plus any child sub-populations.
#[derive(Debug, Clone)]
pub struct Population {
    pub name: String,
    pub event_indices: Vec<usize>,
    pub children: Vec<Rc<RefCell<Population>>>,
    pub gate_summary: Option<String>,
}

impl Population {
    pub fn new(name: &str, indices: Vec<usize>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Population {
            name: name.to_string(),
            event_indices: indices,
            children: Vec::new(),
            gate_summary: None,
        }))
    }

    pub fn add_child(&mut self, child: Rc<RefCell<Self>>) {
        self.children.push(child);
    }

    pub fn count(&self) -> usize {
        self.event_indices.len()
    }

    pub fn set_gate_summary(&mut self, summary: &str) {
        self.gate_summary = Some(summary.to_string());
    }
}

/// A serializable snapshot of a population (for project files).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PopulationSnapshot {
    pub name: String,
    pub count: usize,
    pub gate_summary: Option<String>,
    pub children: Vec<PopulationSnapshot>,
}

impl Population {
    /// Flatten this population and its descendants into snapshots.
    pub fn to_snapshot(root: &Rc<RefCell<Self>>) -> PopulationSnapshot {
        let p = root.borrow();
        PopulationSnapshot {
            name: p.name.clone(),
            count: p.event_indices.len(),
            gate_summary: p.gate_summary.clone(),
            children: p.children.iter().map(Population::to_snapshot).collect(),
        }
    }
}

/// The top-level experiment workspace: samples + population trees + stats.
///
/// Note: `Workspace` and `Sample` are not `Serialize` because they hold
/// `Rc<RefCell<Population>>` and `FcsFile` (the latter lacks a serde impl).
/// Persist a project by serializing `PopulationSnapshot`s and FCS metadata
/// separately.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub samples: HashMap<String, Sample>,
    /// Ordered list of sample IDs for stable display order.
    pub sample_order: Vec<String>,
    /// Computed profiles keyed by (sample_id, population_path).
    pub profiles: HashMap<String, PopulationProfile>,
    /// Configurable column sets for the workspace views (W10).
    pub columns: WorkspaceColumns,
    /// Keyword-based sample groups (W1).
    pub groups: SampleGroups,
}

/// Configurable column sets for workspace views (W10).
///
/// Columns are identified by stable string ids (for example `id`, `$TOT`,
/// `count`). Users can add / remove columns per row kind (sample, population or
/// statistic); the effective sets are consulted by the workspace UI and reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorkspaceColumns {
    /// Columns visible for sample rows.
    pub sample_columns: Vec<String>,
    /// Columns visible for population rows.
    pub population_columns: Vec<String>,
    /// Columns visible for statistic rows.
    pub statistic_columns: Vec<String>,
}

impl WorkspaceColumns {
    /// A fresh set of empty column lists.
    pub fn new() -> Self {
        Self::default()
    }

    /// Is `column` enabled for the given row kind?
    pub fn is_enabled(&self, kind: &str, column: &str) -> bool {
        let columns = match kind {
            "sample" => &self.sample_columns,
            "population" => &self.population_columns,
            "statistic" => &self.statistic_columns,
            _ => return false,
        };
        columns.iter().any(|c| c == column)
    }

    /// Return a copy of these column sets with `column` added to `kind`.
    pub fn with_column(&self, kind: &str, column: &str) -> Self {
        let mut next = self.clone();
        let columns = match kind {
            "sample" => &mut next.sample_columns,
            "population" => &mut next.population_columns,
            "statistic" => &mut next.statistic_columns,
            _ => return next,
        };
        if !columns.iter().any(|c| c == column) {
            columns.push(column.to_string());
        }
        next
    }

    /// Return a copy of these column sets with `column` removed from `kind`.
    pub fn without_column(&self, kind: &str, column: &str) -> Self {
        let mut next = self.clone();
        let columns = match kind {
            "sample" => &mut next.sample_columns,
            "population" => &mut next.population_columns,
            "statistic" => &mut next.statistic_columns,
            _ => return next,
        };
        columns.retain(|c| c != column);
        next
    }
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub id: String,
    pub fcs_file: FcsFile,
    /// Root populations for this sample.
    pub root_populations: Vec<Rc<RefCell<Population>>>,
    /// Gated populations produced by the user (flat list with paths).
    pub gates: Vec<GatedPopulation>,
}

impl Sample {
    pub fn new(id: &str, fcs_file: FcsFile) -> Self {
        Self {
            id: id.to_string(),
            fcs_file,
            root_populations: Vec::new(),
            gates: Vec::new(),
        }
    }
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            samples: HashMap::new(),
            sample_order: Vec::new(),
            profiles: HashMap::new(),
            columns: WorkspaceColumns::new(),
            groups: SampleGroups::new(),
        }
    }

    pub fn add_sample(&mut self, id: &str, fcs_file: FcsFile) {
        let sample = Sample::new(id, fcs_file);
        self.samples.insert(id.to_string(), sample);
        self.sample_order.push(id.to_string());
        self.refresh_groups();
    }

    pub fn get_sample(&self, id: &str) -> Option<&Sample> {
        self.samples.get(id)
    }

    pub fn get_sample_mut(&mut self, id: &str) -> Option<&mut Sample> {
        self.samples.get_mut(id)
    }

    /// Add a root (top-level) population to a sample.
    pub fn add_root_population(
        &mut self,
        sample_id: &str,
        name: &str,
        indices: Vec<usize>,
    ) -> Option<Rc<RefCell<Population>>> {
        let sample = self.get_sample_mut(sample_id)?;
        let pop = Population::new(name, indices);
        sample.root_populations.push(Rc::clone(&pop));
        Some(pop)
    }

    /// Compute and cache the statistical profile for a gated population.
    pub fn compute_profile_for(
        &mut self,
        sample_id: &str,
        population_path: &str,
        indices: &[usize],
    ) -> PopulationProfile {
        self.compute_profile_for_with(sample_id, population_path, indices, &[])
    }

    /// Compute and cache a profile with arbitrary "Freq of" denominators.
    ///
    /// `frequencies` lists reference populations (ancestors, grandparent, the
    /// grand total, or any other named denominator) as
    /// `(label, Some(event_count))`. Each entry becomes a
    /// [`crate::statistics::Statistic::FreqOf`] statistic in the profile.
    pub fn compute_profile_for_with(
        &mut self,
        sample_id: &str,
        population_path: &str,
        indices: &[usize],
        frequencies: &[(String, Option<usize>)],
    ) -> PopulationProfile {
        let sample = self.samples.get(sample_id).expect("sample must exist");
        let fcs = &sample.fcs_file;
        let param_names: Vec<String> = fcs.parameters.iter().map(|p| p.name.clone()).collect();
        let total_count = Some(fcs.event_count);
        let profile = crate::statistics::compute_profile_with_denominators(
            population_path,
            &fcs.data,
            indices,
            &param_names,
            None,
            total_count,
            frequencies,
        );
        let key = format!("{sample_id}::{population_path}");
        self.profiles.insert(key.clone(), profile.clone());
        profile
    }

    /// List all cached profiles.
    pub fn profiles(&self) -> impl Iterator<Item = (&String, &PopulationProfile)> {
        self.profiles.iter()
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcs::FCS_HEADER_LEN;

    #[test]
    fn add_and_compute_profile() {
        let mut ws = Workspace::new();
        let fcs = crate::fcs::FcsParser::parse_bytes(&build_fcs(4, 2)).unwrap();
        ws.add_sample("s1", fcs);

        let indices = vec![0, 1];
        ws.add_root_population("s1", "Live", indices.clone());
        let profile = ws.compute_profile_for("s1", "Live", &indices);
        assert_eq!(profile.count, 2);
        assert!(ws.profiles().count() >= 1);
    }

    #[test]
    fn compute_profile_with_freq_of_ancestor() {
        let mut ws = Workspace::new();
        let fcs = crate::fcs::FcsParser::parse_bytes(&build_fcs(6, 2)).unwrap();
        ws.add_sample("s1", fcs);

        let indices = vec![0, 2, 4];
        ws.add_root_population("s1", "Parent", (0..6).collect());
        ws.add_root_population("s1", "Child", indices.clone());
        // Six events total, so the child is 50% of the grand total.
        let frequencies = vec![("Grandparent".to_string(), Some(6usize))];
        let profile = ws.compute_profile_for_with("s1", "Child", &indices, &frequencies);
        assert_eq!(profile.count, 3);
        let freq = profile
            .stats
            .iter()
            .find(|s| {
                s.statistic
                    == crate::statistics::Statistic::FreqOf {
                        denominator: "Grandparent".to_string(),
                    }
            })
            .expect("grandparent frequency should be computed");
        assert!((freq.value - 50.0).abs() < 1e-6);
        // The cached profile is available through the normal accessor.
        let cached = ws
            .profiles()
            .find(|(key, _)| key.as_str() == "s1::Child")
            .map(|(_, p)| p)
            .unwrap();
        assert_eq!(cached.count, 3);
    }

    #[test]
    fn workspace_columns_default_to_empty_sets() {
        let columns = WorkspaceColumns::new();
        assert!(columns.sample_columns.is_empty());
        assert!(columns.population_columns.is_empty());
        assert!(columns.statistic_columns.is_empty());
        assert!(!columns.is_enabled("sample", "id"));
        assert!(!columns.is_enabled("population", "count"));
        assert!(!columns.is_enabled("statistic", "mean"));
        assert!(!columns.is_enabled("unknown", "id"));
    }

    #[test]
    fn workspace_columns_add_and_remove_columns() {
        let columns = WorkspaceColumns::new();
        let with = columns.with_column("sample", "id");
        assert!(with.is_enabled("sample", "id"));
        assert!(!with.is_enabled("population", "id"));

        let with_tot = with.with_column("sample", "$TOT");
        assert!(with_tot.is_enabled("sample", "$TOT"));
        // Adding an existing column is idempotent.
        assert_eq!(with_tot.with_column("sample", "id"), with_tot);

        let without = with_tot.without_column("sample", "$TOT");
        assert!(!without.is_enabled("sample", "$TOT"));
        assert!(without.is_enabled("sample", "id"));
        let without_id = without.without_column("sample", "id");
        assert!(!without_id.is_enabled("sample", "id"));
    }

    fn build_fcs(events: usize, params: usize) -> Vec<u8> {
        // Minimal valid FCS 3.0 with proper header.
        // Pointers are 0-indexed byte offsets, matching real-world FCS files.
        let mut text = Vec::new();
        text.push(b'/');
        text.extend_from_slice(format!("$TOT/{events}/").as_bytes());
        text.extend_from_slice(format!("$PAR/{params}/").as_bytes());
        for n in 1..=params {
            text.extend_from_slice(format!("$P{n}N/P{n}/").as_bytes());
            text.extend_from_slice(format!("$P{n}R/4.0/").as_bytes());
        }
        text.extend_from_slice("$BYTEORD/1,2,3,4/".to_string().as_bytes());

        let text_start = FCS_HEADER_LEN; // 0-indexed, right after header
        let text_end = text_start + text.len();
        let data_start = text_end;
        let data_end = data_start;
        let analysis_start = data_end + 1;
        let analysis_end = analysis_start + 1;
        fn ptr(n: usize) -> [u8; 8] {
            let s = format!("{n:>8}");
            let mut b = [0u8; 8];
            b.copy_from_slice(s.as_bytes());
            b
        }

        let mut buf = Vec::new();
        buf.extend_from_slice(b"FCS3.0    "); // version (6) + reserved (4) = 10 bytes
        buf.extend_from_slice(&ptr(text_start));
        buf.extend_from_slice(&ptr(text_end));
        buf.extend_from_slice(&ptr(data_start));
        buf.extend_from_slice(&ptr(data_end));
        buf.extend_from_slice(&ptr(analysis_start));
        buf.extend_from_slice(&ptr(analysis_end));
        buf.extend_from_slice(&text);
        for i in 0..events {
            for _ in 0..params {
                buf.extend_from_slice(&(i as f32).to_le_bytes());
            }
        }
        buf
    }
}
