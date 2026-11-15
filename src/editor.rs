//! Workspace metadata editing (Phase 9, W2).
//!
//! Provides a sample/keyword editor, sample / population / group renaming,
//! keyword find-and-replace, keyword-value series creation and
//! copy-keyword-to-group actions.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::groups::keyword_value;
use crate::workspace::{Population, Workspace};

/// Result of a keyword find-and-replace operation (W2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeywordReplaceReport {
    /// Samples whose keyword value changed.
    pub samples_changed: usize,
    /// Keyword entries that changed.
    pub values_changed: usize,
    /// Individual text substitutions performed.
    pub replacements: usize,
}

/// A scoped editor for one sample's keywords (W2).
pub struct SampleEditor<'a> {
    workspace: &'a mut Workspace,
    sample_id: String,
}

impl SampleEditor<'_> {
    pub fn sample_id(&self) -> &str {
        &self.sample_id
    }

    /// Read a keyword case-insensitively.
    pub fn get(&self, keyword: &str) -> Option<&str> {
        self.workspace.get_keyword(&self.sample_id, keyword)
    }

    /// Insert or replace a keyword (stored uppercase).
    pub fn set(&mut self, keyword: &str, value: impl Into<String>) -> bool {
        self.workspace
            .set_keyword(&self.sample_id, keyword, value.into())
    }

    /// Remove a keyword case-insensitively.
    pub fn remove(&mut self, keyword: &str) -> bool {
        self.workspace.remove_keyword(&self.sample_id, keyword)
    }

    /// All keywords for this sample.
    pub fn keywords(&self) -> Option<&HashMap<String, String>> {
        self.workspace.sample_keywords(&self.sample_id)
    }
}

fn replace_ascii_case_insensitive(
    haystack: &str,
    needle: &str,
    replacement: &str,
) -> (String, usize) {
    if needle.is_empty() {
        return (haystack.to_string(), 0);
    }
    if !haystack.is_ascii() || !needle.is_ascii() {
        let count = haystack.matches(needle).count();
        return (haystack.replace(needle, replacement), count);
    }
    let haystack_bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    if needle_bytes.len() > haystack_bytes.len() {
        return (haystack.to_string(), 0);
    }
    let mut out = String::with_capacity(haystack.len());
    let mut cursor = 0;
    let mut count = 0;
    let mut index = 0;
    while index + needle_bytes.len() <= haystack_bytes.len() {
        let candidate = &haystack_bytes[index..index + needle_bytes.len()];
        if candidate.eq_ignore_ascii_case(needle_bytes) {
            out.push_str(&haystack[cursor..index]);
            out.push_str(replacement);
            index += needle_bytes.len();
            cursor = index;
            count += 1;
        } else {
            index += 1;
        }
    }
    out.push_str(&haystack[cursor..]);
    (out, count)
}

fn rename_population_nodes(nodes: &[Rc<RefCell<Population>>], old: &str, new: &str) -> bool {
    let mut renamed = false;
    for node in nodes {
        let mut node_ref = node.borrow_mut();
        if node_ref.name == old {
            node_ref.name = new.to_string();
            renamed = true;
        }
        if rename_population_nodes(&node_ref.children, old, new) {
            renamed = true;
        }
    }
    renamed
}

fn renamed_population_path(path: &str, old: &str, new: &str) -> Option<String> {
    if path == old {
        return Some(new.to_string());
    }
    if let Some(rest) = path.strip_suffix(&format!("/{old}")) {
        return Some(format!("{rest}/{new}"));
    }
    if let Some(rest) = path.strip_suffix(&format!("::{old}")) {
        return Some(format!("{rest}::{new}"));
    }
    None
}

impl Workspace {
    /// Read a keyword case-insensitively (W2).
    pub fn get_keyword<'a>(&'a self, sample_id: &str, keyword: &str) -> Option<&'a str> {
        let sample = self.samples.get(sample_id)?;
        keyword_value(&sample.fcs_file.keywords, keyword)
    }

    /// Insert or replace a sample keyword.
    pub fn set_keyword(
        &mut self,
        sample_id: &str,
        keyword: &str,
        value: impl Into<String>,
    ) -> bool {
        let Some(sample) = self.samples.get_mut(sample_id) else {
            return false;
        };
        sample
            .fcs_file
            .keywords
            .insert(keyword.to_uppercase(), value.into());
        true
    }

    /// Remove a sample keyword case-insensitively.
    pub fn remove_keyword(&mut self, sample_id: &str, keyword: &str) -> bool {
        let Some(sample) = self.samples.get_mut(sample_id) else {
            return false;
        };
        let key = sample
            .fcs_file
            .keywords
            .keys()
            .find(|key| key.eq_ignore_ascii_case(keyword))
            .cloned();
        key.is_some_and(|key| sample.fcs_file.keywords.remove(&key).is_some())
    }

    /// Open a scoped keyword editor for one sample (W2).
    pub fn edit_sample(&mut self, sample_id: &str) -> Option<SampleEditor<'_>> {
        if self.samples.contains_key(sample_id) {
            Some(SampleEditor {
                workspace: self,
                sample_id: sample_id.to_string(),
            })
        } else {
            None
        }
    }

    /// Rename a sample, updating order, cached profiles, group membership and
    /// group gate targets (W2).
    pub fn rename_sample(&mut self, old: &str, new: &str) -> bool {
        if old == new || self.samples.contains_key(new) || !self.samples.contains_key(old) {
            return false;
        }
        let Some(mut sample) = self.samples.remove(old) else {
            return false;
        };
        sample.id = new.to_string();
        self.samples.insert(new.to_string(), sample);
        if let Some(index) = self.sample_order.iter().position(|id| id == old) {
            self.sample_order[index] = new.to_string();
        } else {
            self.sample_order.push(new.to_string());
        }

        let prefix = format!("{old}::");
        let profile_keys: Vec<String> = self
            .profiles
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .cloned()
            .collect();
        for key in profile_keys {
            let Some(profile) = self.profiles.remove(&key) else {
                continue;
            };
            let path = key.strip_prefix(&prefix).unwrap_or("");
            self.profiles.insert(format!("{new}::{path}"), profile);
        }

        for group in &mut self.groups.groups {
            for member in &mut group.members {
                if member == old {
                    *member = new.to_string();
                }
            }
            for gate in &mut group.gates {
                for sample in &mut gate.samples {
                    if sample == old {
                        *sample = new.to_string();
                    }
                }
            }
        }
        true
    }

    /// Rename a population in one sample, including its gates and cached
    /// profiles (W2).
    pub fn rename_population(&mut self, sample_id: &str, old: &str, new: &str) -> bool {
        if old == new {
            return false;
        }
        let Some(sample) = self.samples.get_mut(sample_id) else {
            return false;
        };
        let mut renamed = rename_population_nodes(&sample.root_populations, old, new);
        for gate in &mut sample.gates {
            if gate.name == old {
                gate.name = new.to_string();
                renamed = true;
            }
        }

        let prefix = format!("{sample_id}::");
        let updates: Vec<(String, String)> = self
            .profiles
            .keys()
            .filter_map(|key| {
                let path = key.strip_prefix(&prefix)?;
                let updated = renamed_population_path(path, old, new)?;
                Some((key.clone(), format!("{prefix}{updated}")))
            })
            .collect();
        for (old_key, new_key) in updates {
            if let Some(profile) = self.profiles.remove(&old_key) {
                self.profiles.insert(new_key, profile);
                renamed = true;
            }
        }
        renamed
    }

    /// Rename a sample group (W2).
    pub fn rename_group(&mut self, old: &str, new: &str) -> bool {
        self.groups.rename(old, new)
    }

    /// Find samples that define a keyword, in sample order (W2).
    pub fn find_keyword(&self, keyword: &str) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        for sample_id in &self.sample_order {
            if let Some(value) = self.get_keyword(sample_id, keyword) {
                rows.push((sample_id.clone(), value.to_string()));
            }
        }
        rows
    }

    /// Rename a keyword across every sample (W2). Returns the number of
    /// keyword entries renamed.
    pub fn rename_keyword(&mut self, old: &str, new: &str) -> usize {
        if old.eq_ignore_ascii_case(new) {
            return 0;
        }
        let mut renamed = 0;
        for sample in self.samples.values_mut() {
            let key = sample
                .fcs_file
                .keywords
                .keys()
                .find(|key| key.eq_ignore_ascii_case(old))
                .cloned();
            if let Some(key) = key
                && let Some(value) = sample.fcs_file.keywords.remove(&key)
            {
                sample.fcs_file.keywords.insert(new.to_uppercase(), value);
                renamed += 1;
            }
        }
        renamed
    }

    /// Replace text inside one keyword's values across all samples (W2).
    pub fn replace_keyword_text(
        &mut self,
        keyword: &str,
        search: &str,
        replacement: &str,
        case_sensitive: bool,
    ) -> KeywordReplaceReport {
        let sample_ids: Vec<String> = self
            .sample_order
            .iter()
            .filter(|id| self.samples.contains_key(*id))
            .cloned()
            .chain(
                self.samples
                    .keys()
                    .filter(|id| !self.sample_order.contains(*id))
                    .cloned(),
            )
            .collect();
        self.replace_keyword_text_in_samples(
            &sample_ids,
            keyword,
            search,
            replacement,
            case_sensitive,
        )
    }

    /// Replace text inside one keyword's values for group members (W2).
    pub fn replace_keyword_text_in_group(
        &mut self,
        group_name: &str,
        keyword: &str,
        search: &str,
        replacement: &str,
        case_sensitive: bool,
    ) -> KeywordReplaceReport {
        let members = self
            .groups
            .members(group_name)
            .map(|members| members.to_vec())
            .unwrap_or_default();
        self.replace_keyword_text_in_samples(&members, keyword, search, replacement, case_sensitive)
    }

    fn replace_keyword_text_in_samples(
        &mut self,
        sample_ids: &[String],
        keyword: &str,
        search: &str,
        replacement: &str,
        case_sensitive: bool,
    ) -> KeywordReplaceReport {
        let mut report = KeywordReplaceReport::default();
        for sample_id in sample_ids {
            let Some(sample) = self.samples.get_mut(sample_id) else {
                continue;
            };
            let key = sample
                .fcs_file
                .keywords
                .keys()
                .find(|key| key.eq_ignore_ascii_case(keyword))
                .cloned();
            let Some(key) = key else {
                continue;
            };
            let Some(value) = sample.fcs_file.keywords.get(&key).cloned() else {
                continue;
            };
            let (updated, replacements) = if case_sensitive {
                let count = value.matches(search).count();
                (value.replace(search, replacement), count)
            } else {
                replace_ascii_case_insensitive(&value, search, replacement)
            };
            if replacements > 0 {
                sample.fcs_file.keywords.insert(key, updated);
                report.samples_changed += 1;
                report.values_changed += 1;
                report.replacements += replacements;
            }
        }
        report
    }

    /// Copy a keyword value from one sample to every member of a group (W2).
    pub fn copy_keyword_to_group(
        &mut self,
        group_name: &str,
        keyword: &str,
        source_sample: &str,
    ) -> usize {
        let Some(value) = self.get_keyword(source_sample, keyword).map(str::to_string) else {
            return 0;
        };
        self.set_keyword_for_group(group_name, keyword, value)
    }

    /// Set a keyword to the same value on every group member (W2).
    pub fn set_keyword_for_group(
        &mut self,
        group_name: &str,
        keyword: &str,
        value: impl Into<String>,
    ) -> usize {
        let members = self
            .groups
            .members(group_name)
            .map(|members| members.to_vec())
            .unwrap_or_default();
        let value = value.into();
        members
            .iter()
            .filter(|sample_id| self.set_keyword(sample_id, keyword, value.clone()))
            .count()
    }

    /// Assign an arithmetic series to a keyword across the given samples (W2).
    pub fn create_keyword_series(
        &mut self,
        keyword: &str,
        samples: &[String],
        start: f64,
        step: f64,
    ) -> usize {
        let mut assigned = 0;
        for (index, sample_id) in samples.iter().enumerate() {
            let value = start + step * index as f64;
            let text = if value.fract() == 0.0 {
                format!("{}", value as i64)
            } else {
                format!("{value}")
            };
            if self.set_keyword(sample_id, keyword, text) {
                assigned += 1;
            }
        }
        assigned
    }

    /// Assign an arithmetic keyword series to a group's members in order (W2).
    pub fn create_keyword_series_in_group(
        &mut self,
        group_name: &str,
        keyword: &str,
        start: f64,
        step: f64,
    ) -> usize {
        let members = self
            .groups
            .members(group_name)
            .map(|members| members.to_vec())
            .unwrap_or_default();
        self.create_keyword_series(keyword, &members, start, step)
    }

    /// Assign explicit values from `(sample_id, value)` pairs to a keyword (W2).
    pub fn set_keyword_series(&mut self, keyword: &str, values: &[(String, String)]) -> usize {
        values
            .iter()
            .filter(|(sample_id, value)| self.set_keyword(sample_id, keyword, value.clone()))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gating::GatedPopulation;
    use crate::groups::SampleGroup;
    use crate::workspace::Workspace;
    use ndarray::array;

    fn fcs(keywords: &[(&str, &str)], data: ndarray::Array2<f32>) -> crate::fcs::FcsFile {
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
        crate::fcs::FcsFile {
            version: "FCS3.0".to_string(),
            keywords: map,
            parameters,
            event_count: data.nrows(),
            data,
        }
    }

    fn workspace() -> Workspace {
        let data = array![[1.0, 1.0], [2.0, 2.0], [8.0, 8.0]];
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
    fn sample_keyword_editor_sets_gets_and_removes() {
        let mut workspace = workspace();
        assert_eq!(workspace.get_keyword("s1", "treatment"), Some("A"));
        assert_eq!(workspace.get_keyword("missing", "treatment"), None);
        assert!(workspace.set_keyword("s1", "Treatment", "C"));
        assert_eq!(workspace.get_keyword("s1", "TREATMENT"), Some("C"));
        assert!(workspace.remove_keyword("s1", "treatment"));
        assert_eq!(workspace.get_keyword("s1", "treatment"), None);

        let mut editor = workspace.edit_sample("s2").expect("sample editor");
        assert_eq!(editor.get("patient"), Some("P2"));
        assert!(editor.set("panel", "B"));
        assert_eq!(editor.sample_id(), "s2");
        assert_eq!(
            editor.keywords().unwrap().get("PANEL").map(String::as_str),
            Some("B")
        );
        assert!(workspace.edit_sample("missing").is_none());
    }

    #[test]
    fn rename_sample_updates_order_groups_and_profiles() {
        let mut workspace = workspace();
        workspace.add_group(SampleGroup::by_keyword("Arm A", "treatment", "A"));
        workspace.compute_profile_for("s1", "Live", &[0, 1]);
        assert!(workspace.rename_sample("s1", "sample-1"));
        assert!(workspace.get_sample("s1").is_none());
        assert_eq!(workspace.sample_order[0], "sample-1");
        assert!(workspace.profiles.contains_key("sample-1::Live"));
        assert!(
            workspace
                .group_members("Arm A")
                .unwrap()
                .contains(&"sample-1".to_string())
        );
        assert_eq!(workspace.get_sample("sample-1").unwrap().id, "sample-1");
        assert!(!workspace.rename_sample("sample-1", "s2"));
        assert!(!workspace.rename_sample("missing", "other"));
        assert!(!workspace.rename_sample("sample-1", "sample-1"));
    }

    #[test]
    fn rename_population_and_group() {
        let mut workspace = workspace();
        workspace.add_root_population("s1", "Live", vec![0, 1]);
        workspace.add_root_population("s1", "T cells", vec![0]);
        workspace
            .get_sample_mut("s1")
            .unwrap()
            .gates
            .push(GatedPopulation::new("Live", vec![0, 1], "manual"));
        workspace.compute_profile_for("s1", "Live", &[0, 1]);
        workspace.add_group(SampleGroup::manual("Arm A"));

        assert!(workspace.rename_population("s1", "Live", "Viable"));
        assert!(workspace.profiles.contains_key("s1::Viable"));
        assert!(
            workspace
                .get_sample("s1")
                .unwrap()
                .gates
                .iter()
                .any(|gate| gate.name == "Viable")
        );
        assert!(workspace.rename_group("Arm A", "Arm B"));
        assert!(!workspace.rename_group("Arm B", "Arm B"));
    }

    #[test]
    fn keyword_find_replace_and_copy_to_group() {
        let mut workspace = workspace();
        workspace.add_group(SampleGroup::by_keyword("Arm A", "treatment", "A"));
        assert_eq!(
            workspace.find_keyword("notes"),
            vec![("s1".to_string(), "baseline".to_string())]
        );
        let report = workspace.replace_keyword_text("notes", "BASELINE", "control", false);
        assert_eq!(report.samples_changed, 1);
        assert_eq!(report.replacements, 1);
        assert_eq!(workspace.get_keyword("s1", "NOTES"), Some("control"));
        assert_eq!(workspace.copy_keyword_to_group("Arm A", "notes", "s1"), 2);
        assert_eq!(workspace.get_keyword("s2", "notes"), Some("control"));
        assert_eq!(workspace.set_keyword_for_group("Arm A", "panel", "A"), 2);
        assert_eq!(workspace.get_keyword("s2", "panel"), Some("A"));
        assert_eq!(workspace.rename_keyword("panel", "panel_name"), 2);
        assert_eq!(workspace.get_keyword("s1", "panel_name"), Some("A"));
    }

    #[test]
    fn keyword_series_creation() {
        let mut workspace = workspace();
        workspace.add_group(SampleGroup::by_keyword("Arm A", "treatment", "A"));
        assert_eq!(
            workspace.create_keyword_series_in_group("Arm A", "dilution", 1.0, 2.0),
            2
        );
        assert_eq!(workspace.get_keyword("s1", "dilution"), Some("1"));
        assert_eq!(workspace.get_keyword("s2", "dilution"), Some("3"));
        assert_eq!(
            workspace.create_keyword_series(
                "dose",
                &["s1".to_string(), "s3".to_string()],
                0.5,
                0.5,
            ),
            2
        );
        assert_eq!(workspace.get_keyword("s3", "dose"), Some("1"));
        assert_eq!(
            workspace.set_keyword_series("replicate", &[("s1".to_string(), "R1".to_string())],),
            1
        );
        assert_eq!(workspace.get_keyword("s1", "replicate"), Some("R1"));
    }

    #[test]
    fn keyword_replace_in_group_is_scoped() {
        let mut workspace = workspace();
        workspace.add_group(SampleGroup::by_keyword("Arm A", "treatment", "A"));
        let report =
            workspace.replace_keyword_text_in_group("Arm A", "patient", "P", "Sample ", true);
        assert_eq!(report.samples_changed, 2);
        assert_eq!(workspace.get_keyword("s1", "patient"), Some("Sample 1"));
        assert_eq!(workspace.get_keyword("s3", "patient"), Some("P3"));
    }
}
