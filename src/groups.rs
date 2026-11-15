//! Keyword-based sample groups (Phase 9, W1).
//!
//! Groups can be manual or live (membership recomputed from keyword inclusion
//! criteria). Each group carries a role for analysis defaults, a color/style,
//! and optional group-owned gates that can be synchronised across members.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::fcs::FcsFile;
use crate::gating::{Gate, GateGeometry, GateShape, GatedPopulation};
use crate::statistics::PopulationProfile;
use crate::workspace::{Population, Sample, Workspace};

/// Look up a keyword case-insensitively.
pub fn keyword_value<'a>(keywords: &'a HashMap<String, String>, keyword: &str) -> Option<&'a str> {
    keywords.get(keyword).map(String::as_str).or_else(|| {
        keywords
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(keyword))
            .map(|(_, value)| value.as_str())
    })
}

/// One keyword inclusion criterion for a live sample group (W1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupCriterion {
    Equals {
        keyword: String,
        value: String,
    },
    NotEquals {
        keyword: String,
        value: String,
    },
    Contains {
        keyword: String,
        value: String,
    },
    StartsWith {
        keyword: String,
        value: String,
    },
    Exists {
        keyword: String,
    },
    Missing {
        keyword: String,
    },
    AnyOf {
        keyword: String,
        values: Vec<String>,
    },
    All(Vec<GroupCriterion>),
    Any(Vec<GroupCriterion>),
}

impl GroupCriterion {
    pub fn equals(keyword: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Equals {
            keyword: keyword.into(),
            value: value.into(),
        }
    }

    pub fn not_equals(keyword: impl Into<String>, value: impl Into<String>) -> Self {
        Self::NotEquals {
            keyword: keyword.into(),
            value: value.into(),
        }
    }

    pub fn contains(keyword: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Contains {
            keyword: keyword.into(),
            value: value.into(),
        }
    }

    pub fn starts_with(keyword: impl Into<String>, value: impl Into<String>) -> Self {
        Self::StartsWith {
            keyword: keyword.into(),
            value: value.into(),
        }
    }

    pub fn exists(keyword: impl Into<String>) -> Self {
        Self::Exists {
            keyword: keyword.into(),
        }
    }

    pub fn missing(keyword: impl Into<String>) -> Self {
        Self::Missing {
            keyword: keyword.into(),
        }
    }

    pub fn any_of<I, S>(keyword: impl Into<String>, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::AnyOf {
            keyword: keyword.into(),
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    pub fn all(criteria: Vec<GroupCriterion>) -> Self {
        Self::All(criteria)
    }

    pub fn any(criteria: Vec<GroupCriterion>) -> Self {
        Self::Any(criteria)
    }

    /// Test this criterion against a sample's FCS keywords.
    pub fn matches(&self, keywords: &HashMap<String, String>) -> bool {
        match self {
            Self::Equals { keyword, value } => {
                keyword_value(keywords, keyword) == Some(value.as_str())
            }
            Self::NotEquals { keyword, value } => {
                keyword_value(keywords, keyword) != Some(value.as_str())
            }
            Self::Contains { keyword, value } => keyword_value(keywords, keyword)
                .is_some_and(|candidate| candidate.to_lowercase().contains(&value.to_lowercase())),
            Self::StartsWith { keyword, value } => keyword_value(keywords, keyword)
                .is_some_and(|candidate| candidate.starts_with(value.as_str())),
            Self::Exists { keyword } => keyword_value(keywords, keyword).is_some(),
            Self::Missing { keyword } => keyword_value(keywords, keyword).is_none(),
            Self::AnyOf { keyword, values } => keyword_value(keywords, keyword)
                .is_some_and(|candidate| values.iter().any(|value| value == candidate)),
            Self::All(criteria) => criteria.iter().all(|criterion| criterion.matches(keywords)),
            Self::Any(criteria) => criteria.iter().any(|criterion| criterion.matches(keywords)),
        }
    }
}

/// Analysis role assigned to a sample group (W1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GroupRole {
    #[default]
    Unassigned,
    /// Baseline / untreated control.
    Baseline,
    /// Compensation control.
    Compensation,
    /// Gate hierarchy level.
    GatingLevel,
    /// Replicate set.
    Replicate,
    /// Test / experimental condition.
    Test,
}

impl GroupRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unassigned => "unassigned",
            Self::Baseline => "baseline",
            Self::Compensation => "compensation",
            Self::GatingLevel => "gating level",
            Self::Replicate => "replicate",
            Self::Test => "test",
        }
    }
}

/// Line style used when group gates are drawn (W1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GroupLineStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

impl GroupLineStyle {
    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Dashed => "dashed",
            Self::Dotted => "dotted",
        }
    }

    pub fn dash_array(self) -> Option<&'static str> {
        match self {
            Self::Solid => None,
            Self::Dashed => Some("6 3"),
            Self::Dotted => Some("2 3"),
        }
    }
}

/// Marker shape used for group samples in figures (W1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GroupMarker {
    #[default]
    None,
    Circle,
    Square,
    Triangle,
    Diamond,
}

impl GroupMarker {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Triangle => "triangle",
            Self::Diamond => "diamond",
        }
    }
}

/// Color and style for a sample group (W1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupStyle {
    pub color: String,
    pub line_style: GroupLineStyle,
    pub marker: GroupMarker,
    pub line_width: f32,
    pub visible: bool,
}

impl Default for GroupStyle {
    fn default() -> Self {
        Self {
            color: "#4a90d9".to_string(),
            line_style: GroupLineStyle::Solid,
            marker: GroupMarker::None,
            line_width: 1.0,
            visible: true,
        }
    }
}

impl GroupStyle {
    pub fn new(color: impl Into<String>) -> Self {
        Self {
            color: color.into(),
            ..Self::default()
        }
    }

    pub fn with_line_style(mut self, line_style: GroupLineStyle) -> Self {
        self.line_style = line_style;
        self
    }

    pub fn with_marker(mut self, marker: GroupMarker) -> Self {
        self.marker = marker;
        self
    }

    pub fn with_line_width(mut self, line_width: f32) -> Self {
        self.line_width = line_width.max(0.0);
        self
    }

    pub fn with_visibility(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
}

/// A gate owned by a group, optionally synchronized across all members (W1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupGate {
    pub name: String,
    pub param_idx_x: usize,
    pub param_idx_y: usize,
    pub geometry: GateGeometry,
    /// Explicit sample targets; empty means all group members.
    pub samples: Vec<String>,
    /// Apply this gate to every group member.
    pub synchronized: bool,
}

impl GroupGate {
    pub fn new(
        name: impl Into<String>,
        param_idx_x: usize,
        param_idx_y: usize,
        geometry: GateGeometry,
    ) -> Self {
        Self {
            name: name.into(),
            param_idx_x,
            param_idx_y,
            geometry,
            samples: Vec::new(),
            synchronized: true,
        }
    }

    pub fn rectangle(
        name: impl Into<String>,
        param_idx_x: usize,
        param_idx_y: usize,
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
    ) -> Self {
        Self::new(
            name,
            param_idx_x,
            param_idx_y,
            GateGeometry::rectangle(min_x, max_x, min_y, max_y),
        )
    }

    pub fn ellipse(
        name: impl Into<String>,
        param_idx_x: usize,
        param_idx_y: usize,
        center_x: f32,
        center_y: f32,
        radius_x: f32,
        radius_y: f32,
    ) -> Self {
        Self::new(
            name,
            param_idx_x,
            param_idx_y,
            GateGeometry::ellipse(center_x, center_y, radius_x, radius_y),
        )
    }

    pub fn polygon(
        name: impl Into<String>,
        param_idx_x: usize,
        param_idx_y: usize,
        vertices: Vec<(f32, f32)>,
    ) -> Self {
        Self::new(
            name,
            param_idx_x,
            param_idx_y,
            GateGeometry::polygon(vertices),
        )
    }

    pub fn with_samples(mut self, samples: Vec<String>) -> Self {
        self.samples = samples;
        self.synchronized = false;
        self
    }

    pub fn with_synchronized(mut self, synchronized: bool) -> Self {
        self.synchronized = synchronized;
        self
    }

    /// Apply this gate to a sample's FCS data (W1).
    pub fn evaluate(&self, fcs: &FcsFile) -> GatedPopulation {
        let indices = match self.geometry.shape() {
            GateShape::Rectangle => self
                .geometry
                .to_rectangular_gate(self.param_idx_x, self.param_idx_y)
                .filter(&fcs.data),
            GateShape::Ellipse => self
                .geometry
                .to_ellipse_gate(self.param_idx_x, self.param_idx_y)
                .filter(&fcs.data),
            GateShape::Polygon => self
                .geometry
                .to_polygon_gate(self.param_idx_x, self.param_idx_y)
                .filter(&fcs.data),
        };
        GatedPopulation::new(&self.name, indices, &self.summary())
    }

    /// Human-readable gate description.
    pub fn summary(&self) -> String {
        let (min_x, max_x, min_y, max_y) = self.geometry.bounding_box();
        format!(
            "{} gate '{}' x[{min_x:.2},{max_x:.2}] y[{min_y:.2},{max_y:.2}]",
            self.geometry.shape().label(),
            self.name
        )
    }
}

/// A sample group with keyword criteria, role, style and gates (W1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SampleGroup {
    pub name: String,
    /// Inclusion criteria combined with AND; empty for manual groups.
    pub criteria: Vec<GroupCriterion>,
    /// Live groups recompute membership from `criteria`.
    pub live: bool,
    /// Current member sample ids.
    pub members: Vec<String>,
    pub role: GroupRole,
    pub style: GroupStyle,
    pub gates: Vec<GroupGate>,
    /// Optional parent group (useful for gating levels).
    pub parent: Option<String>,
}

impl SampleGroup {
    /// A manually managed group.
    pub fn manual(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            criteria: Vec::new(),
            live: false,
            members: Vec::new(),
            role: GroupRole::Unassigned,
            style: GroupStyle::default(),
            gates: Vec::new(),
            parent: None,
        }
    }

    /// A live group whose membership is derived from keyword criteria.
    pub fn live(name: impl Into<String>, criteria: Vec<GroupCriterion>) -> Self {
        Self {
            criteria,
            live: true,
            ..Self::manual(name)
        }
    }

    /// A live group matching one keyword value.
    pub fn by_keyword(
        name: impl Into<String>,
        keyword: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self::live(name, vec![GroupCriterion::equals(keyword, value)])
    }

    pub fn with_members(mut self, members: Vec<String>) -> Self {
        self.members = members;
        self
    }

    pub fn with_role(mut self, role: GroupRole) -> Self {
        self.role = role;
        self
    }

    pub fn with_style(mut self, style: GroupStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_parent(mut self, parent: impl Into<String>) -> Self {
        self.parent = Some(parent.into());
        self
    }

    pub fn add_gate(&mut self, gate: GroupGate) -> usize {
        self.gates.push(gate);
        self.gates.len() - 1
    }

    /// True when all inclusion criteria match.
    pub fn matches(&self, keywords: &HashMap<String, String>) -> bool {
        !self.criteria.is_empty()
            && self
                .criteria
                .iter()
                .all(|criterion| criterion.matches(keywords))
    }

    /// Recompute live membership; returns true when the member list changed.
    fn recompute_members(&mut self, samples: &HashMap<String, Sample>, order: &[String]) -> bool {
        if !self.live {
            return false;
        }
        let mut members: Vec<String> = order
            .iter()
            .filter(|id| {
                samples
                    .get(*id)
                    .is_some_and(|sample| self.matches(&sample.fcs_file.keywords))
            })
            .cloned()
            .collect();
        let mut extras: Vec<String> = samples
            .iter()
            .filter(|(id, sample)| !members.contains(id) && self.matches(&sample.fcs_file.keywords))
            .map(|(id, _)| id.clone())
            .collect();
        extras.sort();
        members.extend(extras);
        if members != self.members {
            self.members = members;
            true
        } else {
            false
        }
    }
}

/// Registry of sample groups attached to a workspace (W1).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SampleGroups {
    pub groups: Vec<SampleGroup>,
}

impl SampleGroups {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, group: SampleGroup) -> usize {
        self.groups.push(group);
        self.groups.len() - 1
    }

    /// Add an empty manual group by name.
    pub fn add_manual(&mut self, name: impl Into<String>) -> usize {
        self.add(SampleGroup::manual(name))
    }

    pub fn get(&self, name: &str) -> Option<&SampleGroup> {
        self.groups.iter().find(|group| group.name == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut SampleGroup> {
        self.groups.iter_mut().find(|group| group.name == name)
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.groups.len();
        self.groups.retain(|group| group.name != name);
        self.groups.len() != before
    }

    /// Rename a group; fails if the new name is already used.
    pub fn rename(&mut self, old: &str, new: &str) -> bool {
        if old == new || self.get(new).is_some() {
            return false;
        }
        match self.get_mut(old) {
            Some(group) => {
                group.name = new.to_string();
                for group in &mut self.groups {
                    if group.parent.as_deref() == Some(old) {
                        group.parent = Some(new.to_string());
                    }
                }
                true
            }
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.groups.len()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub fn names(&self) -> Vec<&str> {
        self.groups
            .iter()
            .map(|group| group.name.as_str())
            .collect()
    }

    /// Refresh every live group; returns the number of changed groups.
    pub fn refresh(&mut self, samples: &HashMap<String, Sample>, order: &[String]) -> usize {
        let mut changed = 0;
        for group in &mut self.groups {
            if group.recompute_members(samples, order) {
                changed += 1;
            }
        }
        changed
    }

    pub fn members(&self, name: &str) -> Option<&[String]> {
        self.get(name).map(|group| group.members.as_slice())
    }

    pub fn groups_for_sample(&self, sample_id: &str) -> Vec<&SampleGroup> {
        self.groups
            .iter()
            .filter(|group| group.members.iter().any(|member| member == sample_id))
            .collect()
    }

    pub fn group_for_role(&self, role: GroupRole) -> Option<&SampleGroup> {
        self.groups.iter().find(|group| group.role == role)
    }

    pub fn style_for_sample(&self, sample_id: &str) -> Option<&GroupStyle> {
        self.groups_for_sample(sample_id)
            .first()
            .map(|group| &group.style)
    }
}

fn clone_population(source: &Rc<RefCell<Population>>) -> Rc<RefCell<Population>> {
    let source_ref = source.borrow();
    let copy = Population::new(&source_ref.name, source_ref.event_indices.clone());
    {
        let mut copy_ref = copy.borrow_mut();
        copy_ref.gate_summary = source_ref.gate_summary.clone();
        for child in &source_ref.children {
            copy_ref.add_child(clone_population(child));
        }
    }
    copy
}

impl Workspace {
    /// Recompute live group membership from sample keywords (W1).
    pub fn refresh_groups(&mut self) -> usize {
        self.groups.refresh(&self.samples, &self.sample_order)
    }

    /// Add a group and immediately refresh live membership.
    pub fn add_group(&mut self, group: SampleGroup) -> usize {
        let index = self.groups.add(group);
        self.refresh_groups();
        index
    }

    pub fn group_members(&self, name: &str) -> Option<&[String]> {
        self.groups.members(name)
    }

    pub fn groups_for_sample(&self, sample_id: &str) -> Vec<&SampleGroup> {
        self.groups.groups_for_sample(sample_id)
    }

    pub fn group_for_role(&self, role: GroupRole) -> Option<&SampleGroup> {
        self.groups.group_for_role(role)
    }

    pub fn sample_keywords(&self, sample_id: &str) -> Option<&HashMap<String, String>> {
        self.samples
            .get(sample_id)
            .map(|sample| &sample.fcs_file.keywords)
    }

    /// Copy gates, root populations and cached profiles from `source_id` to
    /// every other member of `group_name` (W1 copy-analysis-to-group).
    ///
    /// Returns the number of analyses copied.
    pub fn copy_analysis_to_group(&mut self, group_name: &str, source_id: &str) -> usize {
        let Some(group) = self.groups.get(group_name) else {
            return 0;
        };
        let members = group.members.clone();
        let Some(source) = self.samples.get(source_id) else {
            return 0;
        };
        let gates = source.gates.clone();
        let roots = source.root_populations.clone();
        let prefix = format!("{source_id}::");
        let profiles: Vec<(String, PopulationProfile)> = self
            .profiles
            .iter()
            .filter_map(|(key, profile)| {
                key.strip_prefix(&prefix)
                    .map(|path| (path.to_string(), profile.clone()))
            })
            .collect();

        let mut copied = 0;
        for member in members {
            if member == source_id {
                continue;
            }
            let Some(target) = self.samples.get_mut(&member) else {
                continue;
            };
            for gate in &gates {
                if !target
                    .gates
                    .iter()
                    .any(|existing| existing.name == gate.name)
                {
                    target.gates.push(gate.clone());
                    copied += 1;
                }
            }
            for root in &roots {
                let name = root.borrow().name.clone();
                if !target
                    .root_populations
                    .iter()
                    .any(|existing| existing.borrow().name == name)
                {
                    target.root_populations.push(clone_population(root));
                    copied += 1;
                }
            }
            for (path, profile) in &profiles {
                let key = format!("{member}::{path}");
                if let std::collections::hash_map::Entry::Vacant(e) = self.profiles.entry(key) {
                    e.insert(profile.clone());
                    copied += 1;
                }
            }
        }
        copied
    }

    /// Evaluate and attach all group-owned/synchronized gates to their targets
    /// (W1). Returns the number of populations added.
    pub fn apply_group_gates(&mut self, group_name: &str) -> usize {
        let Some(group) = self.groups.get(group_name).cloned() else {
            return 0;
        };
        let mut applied = 0;
        for gate in &group.gates {
            let targets = if gate.synchronized || gate.samples.is_empty() {
                group.members.clone()
            } else {
                gate.samples.clone()
            };
            for sample_id in targets {
                let Some(sample) = self.samples.get(&sample_id) else {
                    continue;
                };
                let population = gate.evaluate(&sample.fcs_file);
                let target = self.samples.get_mut(&sample_id).expect("sample exists");
                if !target.gates.iter().any(|existing| {
                    existing.name == population.name
                        && existing.gate_summary == population.gate_summary
                }) {
                    target.gates.push(population);
                    applied += 1;
                }
            }
        }
        applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    fn fcs(keywords: &[(&str, &str)], data: ndarray::Array2<f32>) -> FcsFile {
        let mut map = HashMap::new();
        for (key, value) in keywords {
            map.insert(key.to_uppercase(), value.to_string());
        }
        let parameters = (0..data.ncols())
            .map(|index| crate::fcs::Parameter {
                name: format!("P{index}"),
                description: String::new(),
                range: 1024.0,
                exponent: 0.0,
                min: 0.0,
                max: 1024.0,
            })
            .collect();
        FcsFile {
            version: "FCS3.0".to_string(),
            keywords: map,
            parameters,
            event_count: data.nrows(),
            data,
        }
    }

    fn workspace() -> Workspace {
        let data = array![[1.0, 1.0], [2.0, 2.0], [8.0, 8.0], [9.0, 9.0],];
        let mut workspace = Workspace::new();
        workspace.add_sample(
            "s1",
            fcs(
                &[("TREATMENT", "A"), ("PATIENT", "P1"), ("NOTES", "baseline")],
                data.clone(),
            ),
        );
        workspace.add_sample(
            "s2",
            fcs(&[("TREATMENT", "A"), ("PATIENT", "P2")], data.clone()),
        );
        workspace.add_sample("s3", fcs(&[("TREATMENT", "B"), ("PATIENT", "P3")], data));
        workspace
    }

    #[test]
    fn criteria_match_keywords_case_insensitively() {
        let mut keywords = HashMap::new();
        keywords.insert("TREATMENT".to_string(), "A".to_string());
        keywords.insert("NOTES".to_string(), "baseline sample".to_string());
        assert!(GroupCriterion::equals("treatment", "A").matches(&keywords));
        assert!(!GroupCriterion::equals("treatment", "B").matches(&keywords));
        assert!(GroupCriterion::not_equals("treatment", "B").matches(&keywords));
        assert!(GroupCriterion::contains("notes", "BASELINE").matches(&keywords));
        assert!(GroupCriterion::starts_with("notes", "base").matches(&keywords));
        assert!(GroupCriterion::exists("treatment").matches(&keywords));
        assert!(GroupCriterion::missing("missing").matches(&keywords));
        assert!(GroupCriterion::any_of("treatment", ["A", "C"]).matches(&keywords));
        assert!(
            GroupCriterion::all(vec![
                GroupCriterion::equals("treatment", "A"),
                GroupCriterion::exists("notes"),
            ])
            .matches(&keywords)
        );
        assert!(
            !GroupCriterion::any(vec![
                GroupCriterion::equals("treatment", "B"),
                GroupCriterion::exists("missing"),
            ])
            .matches(&keywords)
        );
    }

    #[test]
    fn live_groups_refresh_from_keywords() {
        let mut workspace = workspace();
        workspace.add_group(SampleGroup::live(
            "Arm A",
            vec![GroupCriterion::equals("treatment", "A")],
        ));
        assert_eq!(workspace.group_members("Arm A").unwrap(), ["s1", "s2"]);
        workspace
            .get_sample_mut("s3")
            .unwrap()
            .fcs_file
            .keywords
            .insert("TREATMENT".to_string(), "A".to_string());
        assert_eq!(workspace.refresh_groups(), 1);
        assert_eq!(
            workspace.group_members("Arm A").unwrap(),
            ["s1", "s2", "s3"]
        );
        assert_eq!(workspace.groups_for_sample("s3").len(), 1);
    }

    #[test]
    fn manual_groups_roles_and_styles() {
        let mut workspace = workspace();
        let mut group = SampleGroup::manual("Control")
            .with_members(vec!["s1".to_string()])
            .with_role(GroupRole::Baseline)
            .with_style(
                GroupStyle::new("#ff0000")
                    .with_marker(GroupMarker::Circle)
                    .with_line_style(GroupLineStyle::Dashed),
            );
        group.add_gate(GroupGate::rectangle("CD3+", 0, 1, 0.0, 5.0, 0.0, 5.0));
        workspace.add_group(group);
        assert_eq!(workspace.group_members("Control").unwrap(), ["s1"]);
        assert_eq!(
            workspace
                .group_for_role(GroupRole::Baseline)
                .unwrap()
                .style
                .color,
            "#ff0000"
        );
        assert_eq!(
            workspace
                .group_for_role(GroupRole::Baseline)
                .unwrap()
                .style
                .marker,
            GroupMarker::Circle
        );
        assert_eq!(workspace.groups_for_sample("s1").len(), 1);
        assert_eq!(workspace.groups_for_sample("s2").len(), 0);
        assert!(workspace.groups.rename("Control", "Baseline"));
        assert!(workspace.groups.get("Baseline").is_some());
        assert!(!workspace.groups.rename("Baseline", "Baseline"));
    }

    #[test]
    fn group_gates_and_copy_analysis() {
        let mut workspace = workspace();
        let mut group = SampleGroup::by_keyword("Arm A", "treatment", "A")
            .with_style(GroupStyle::new("#00aa00"));
        group.add_gate(
            GroupGate::rectangle("CD3+", 0, 1, 0.0, 5.0, 0.0, 5.0).with_synchronized(true),
        );
        workspace.add_group(group);

        assert_eq!(workspace.apply_group_gates("Arm A"), 2);
        let first = workspace.get_sample("s1").unwrap();
        assert_eq!(first.gates.len(), 1);
        assert_eq!(first.gates[0].count(), 2);
        assert_eq!(workspace.apply_group_gates("Arm A"), 0);

        let indices = vec![0, 1];
        workspace.compute_profile_for("s1", "Live", &indices);
        workspace
            .get_sample_mut("s1")
            .unwrap()
            .gates
            .push(GatedPopulation::new("Live", indices, "manual"));
        let copied = workspace.copy_analysis_to_group("Arm A", "s1");
        assert_eq!(copied, 2);
        assert!(
            workspace
                .get_sample("s2")
                .unwrap()
                .gates
                .iter()
                .any(|gate| gate.name == "Live")
        );
        assert!(workspace.profiles.contains_key("s2::Live"));
        // Copying again is idempotent.
        assert_eq!(workspace.copy_analysis_to_group("Arm A", "s1"), 0);
    }

    #[test]
    fn sample_groups_serialize_round_trip() {
        let mut groups = SampleGroups::new();
        groups.add(
            SampleGroup::by_keyword("Arm A", "treatment", "A")
                .with_role(GroupRole::Test)
                .with_style(GroupStyle::new("#123456").with_marker(GroupMarker::Diamond))
                .with_parent("Study"),
        );
        let json = serde_json::to_string(&groups).expect("serialize groups");
        assert!(json.contains("Arm A"));
        let restored: SampleGroups = serde_json::from_str(&json).expect("deserialize groups");
        assert_eq!(restored, groups);
    }
}
