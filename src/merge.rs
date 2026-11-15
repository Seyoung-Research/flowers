//! Sample merge/concatenation, population export and FCS writing (Phase 9, W5).
//!
//! - [`FcsBuilder`] accumulates keywords, parameters and event values and
//!   emits FCS 3.0 files (text-to-FCS conversion).
//! - [`fcs_to_bytes`] / [`write_fcs`] serialise an [`FcsFile`] to disk.
//! - [`merge_fcs`] concatenates event sets from several samples, aligning
//!   channels by parameter name when requested.
//! - Population export turns gated populations of a workspace sample into
//!   [`PopulationRow`]s (CSV-ready).

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::export::PopulationRow;
use crate::fcs::{FcsError, FcsFile, Parameter};
use crate::workspace::Workspace;

/// Errors produced while merging samples or writing FCS files.
#[derive(Debug, Error)]
pub enum MergeError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("FCS error: {0}")]
    Fcs(#[from] FcsError),
    #[error("merge error: {0}")]
    Merge(String),
}

/// Options controlling how several samples are merged into one FCS file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MergeOptions {
    /// Align merged channels by (case-insensitive) parameter name instead of
    /// assuming identical layouts across files.
    pub align_by_name: bool,
    /// Keyword overrides applied to the merged file (for example `$FIL`).
    pub keyword_overrides: HashMap<String, String>,
}

impl MergeOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_align_by_name(mut self, align_by_name: bool) -> Self {
        self.align_by_name = align_by_name;
        self
    }

    pub fn with_keyword_override<K: Into<String>, V: Into<String>>(
        mut self,
        key: K,
        value: V,
    ) -> Self {
        self.keyword_overrides.insert(key.into(), value.into());
        self
    }
}

/// Accumulator for building an FCS file from keywords, parameters and events.
///
/// This is the text-to-FCS conversion path used by sample merges and workspace
/// import: the caller describes the metadata, then feeds event values in row
/// major order (`events * channels` floats, one row per event).
#[derive(Debug, Clone, Default)]
pub struct FcsBuilder {
    pub version: String,
    pub keywords: HashMap<String, String>,
    pub parameters: Vec<Parameter>,
    pub events: Vec<f32>,
}

impl FcsBuilder {
    pub fn new(version: &str) -> Self {
        Self {
            version: version.to_string(),
            keywords: HashMap::new(),
            parameters: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn with_version(mut self, version: &str) -> Self {
        self.version = version.to_string();
        self
    }

    pub fn add_keyword<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
        self.keywords.insert(key.into(), value.into());
    }

    pub fn add_parameter(&mut self, parameter: Parameter) {
        self.parameters.push(parameter);
    }

    pub fn add_events<I: IntoIterator<Item = f32>>(&mut self, events: I) {
        self.events.extend(events);
    }

    pub fn event_count(&self) -> usize {
        let channels = self.parameters.len().max(1);
        self.events.len() / channels
    }

    /// Finalise the accumulator into an [`FcsFile`].
    pub fn build(&self) -> Result<FcsFile, MergeError> {
        let channels = self.parameters.len().max(1);
        let event_count = self.events.len() / channels;
        let data = ndarray::Array2::<f32>::from_shape_fn(
            (event_count, self.parameters.len()),
            |(event, channel)| self.events[event * channels + channel],
        );
        let mut keywords = self.keywords.clone();
        if !keywords.contains_key("$TOT") {
            keywords.insert("$TOT".to_string(), event_count.to_string());
        }
        if !keywords.contains_key("$PAR") {
            keywords.insert("$PAR".to_string(), self.parameters.len().to_string());
        }
        if !keywords.contains_key("$BYTEORD") {
            keywords.insert("$BYTEORD".to_string(), "1,2,3,4".to_string());
        }
        if !keywords.contains_key("$DATATYPE") {
            keywords.insert("$DATATYPE".to_string(), "F".to_string());
        }
        Ok(FcsFile {
            version: self.version.clone(),
            keywords,
            parameters: self.parameters.clone(),
            event_count,
            data,
        })
    }
}

fn ptr(value: u64) -> [u8; 8] {
    let text = format!("{value:>8}");
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(text.as_bytes());
    bytes
}

/// Serialise an [`FcsFile`] into FCS 3.0 bytes (TEXT + binary segments).
pub fn fcs_to_bytes(fcs: &FcsFile) -> Result<Vec<u8>, MergeError> {
    let mut text = String::from("/");
    let mut keys: Vec<&String> = fcs.keywords.keys().collect();
    keys.sort();
    for key in keys {
        if let Some(value) = fcs.keywords.get(key) {
            text.push_str(&format!("{key}/{value}/"));
        }
    }
    if !fcs.keywords.contains_key("$TOT") {
        text.push_str("$TOT/0\n");
    }
    if !fcs.keywords.contains_key("$PAR") {
        text.push_str("$PAR/0\n");
    }
    if !fcs.keywords.contains_key("$BYTEORD") {
        text.push_str("$BYTEORD/1,2,3,4\n");
    }
    if !fcs.keywords.contains_key("$DATATYPE") {
        text.push_str("$DATATYPE/F\n");
    }

    let text_start = 58_u64;
    let text_end = text_start + text.len() as u64;
    let mut data = Vec::with_capacity(fcs.data.len() * 4);
    for row in fcs.data.rows() {
        for value in row.iter() {
            data.extend_from_slice(&value.to_le_bytes());
        }
    }
    let data_start = text_end;
    let data_end = data_start + data.len() as u64;
    let analysis_start = data_end + 1;
    let analysis_end = analysis_start + 1;

    let mut buffer = Vec::with_capacity(58 + text.len() + data.len() + 16);
    buffer.extend_from_slice(b"FCS3.0    ");
    buffer.extend_from_slice(&ptr(text_start));
    buffer.extend_from_slice(&ptr(text_end));
    buffer.extend_from_slice(&ptr(data_start));
    buffer.extend_from_slice(&ptr(data_end));
    buffer.extend_from_slice(&ptr(analysis_start));
    buffer.extend_from_slice(&ptr(analysis_end));
    buffer.extend_from_slice(text.as_bytes());
    buffer.extend_from_slice(&data);
    Ok(buffer)
}

/// Write an [`FcsFile`] to `path` as an FCS 3.0 file.
pub fn write_fcs(fcs: &FcsFile, path: &Path) -> Result<(), MergeError> {
    let bytes = fcs_to_bytes(fcs)?;
    fs::write(path, bytes)?;
    Ok(())
}

/// Concatenate the event sets of several FCS files into one merged FCS file.
///
/// With `align_by_name` the merged file contains the union of the parameter
/// names (first occurrence wins), otherwise channels are assumed to line up
/// positionally with the first file.
pub fn merge_fcs(files: &[FcsFile], options: &MergeOptions) -> Result<FcsFile, MergeError> {
    if files.is_empty() {
        return Err(MergeError::Merge(
            "cannot merge an empty sample set".to_string(),
        ));
    }

    let mut merged_params: Vec<Parameter> = Vec::new();
    let mut mappings: Vec<Vec<usize>> = Vec::new();
    let mut event_counts: Vec<usize> = Vec::new();

    if options.align_by_name {
        let mut index: HashMap<String, usize> = HashMap::new();
        for fcs in files {
            let mut mapping = Vec::with_capacity(fcs.parameters.len());
            for parameter in &fcs.parameters {
                let key = parameter.name.to_ascii_lowercase();
                let target = *index.entry(key).or_insert_with(|| {
                    let target = merged_params.len();
                    merged_params.push(parameter.clone());
                    target
                });
                mapping.push(target);
            }
            mappings.push(mapping);
            event_counts.push(fcs.event_count);
        }
    } else {
        let first = &files[0];
        merged_params = first.parameters.clone();
        for fcs in files {
            let source_count = fcs.parameters.len().min(merged_params.len());
            mappings.push((0..source_count).collect());
            event_counts.push(fcs.event_count);
        }
    }

    let n_channels = merged_params.len();
    let total_events: usize = event_counts.iter().sum();
    let mut merged_events: Vec<f32> = Vec::with_capacity(total_events * n_channels.max(1));
    for (file_index, fcs) in files.iter().enumerate() {
        let mapping = &mappings[file_index];
        for source_event in 0..fcs.event_count {
            let row = fcs.data.row(source_event);
            let mut merged_row = vec![0.0_f32; n_channels];
            for (source, target) in mapping.iter().enumerate() {
                merged_row[*target] = row[source];
            }
            for value in merged_row.iter() {
                merged_events.push(*value);
            }
        }
    }

    let data =
        ndarray::Array2::<f32>::from_shape_fn((total_events, n_channels), |(event, channel)| {
            merged_events[event * n_channels.max(1) + channel]
        });

    let mut keywords = files[0].keywords.clone();
    for (key, value) in &options.keyword_overrides {
        keywords.insert(key.clone(), value.clone());
    }
    keywords.insert("$TOT".to_string(), total_events.to_string());
    keywords.insert("$PAR".to_string(), n_channels.to_string());
    if !keywords.contains_key("$BYTEORD") {
        keywords.insert("$BYTEORD".to_string(), "1,2,3,4".to_string());
    }
    if !keywords.contains_key("$DATATYPE") {
        keywords.insert("$DATATYPE".to_string(), "F".to_string());
    }

    Ok(FcsFile {
        version: "FCS3.0".to_string(),
        keywords,
        parameters: merged_params,
        event_count: total_events,
        data,
    })
}

/// Merge the samples listed in `sample_ids` of a workspace into one new sample.
pub fn merge_workspace_samples(
    workspace: &mut Workspace,
    sample_ids: &[String],
) -> Result<String, MergeError> {
    let files: Vec<FcsFile> = sample_ids
        .iter()
        .map(|id| {
            workspace
                .samples
                .get(id)
                .map(|sample| sample.fcs_file.clone())
                .ok_or_else(|| MergeError::Merge(format!("sample {id} not found")))
        })
        .collect::<Result<_, MergeError>>()?;
    let merged = merge_fcs(&files, &MergeOptions::new())?;
    let merged_id = format!("Merged ({})", files.len());
    workspace.add_sample(&merged_id, merged);
    Ok(merged_id)
}

/// Export the gated populations of one workspace sample as CSV-ready rows.
pub fn population_rows_for_sample(workspace: &Workspace, sample_id: &str) -> Vec<PopulationRow> {
    let Some(sample) = workspace.samples.get(sample_id) else {
        return Vec::new();
    };
    let total = sample.fcs_file.event_count;
    sample
        .gates
        .iter()
        .map(|gate| PopulationRow {
            sample: sample_id.to_string(),
            population: gate.name.clone(),
            parent: None,
            count: gate.count(),
            total,
            parent_count: None,
            gate_summary: Some(gate.gate_summary.clone()),
        })
        .collect()
}

/// Export gated populations of every sample in a workspace as CSV-ready rows.
pub fn population_rows(workspace: &Workspace) -> Vec<PopulationRow> {
    let mut rows = Vec::new();
    for id in &workspace.sample_order {
        rows.extend(population_rows_for_sample(workspace, id));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcs::{FCS_HEADER_LEN, FcsParser};

    fn build_fcs(events: usize, params: usize, extra: &[(&str, &str)]) -> Vec<u8> {
        let mut text = Vec::new();
        text.push(b'/');
        text.extend_from_slice(format!("$TOT/{events}/").as_bytes());
        text.extend_from_slice(format!("$PAR/{params}/").as_bytes());
        for n in 1..=params {
            text.extend_from_slice(format!("$P{n}N/P{n}/").as_bytes());
            text.extend_from_slice(format!("$P{n}R/4.0/").as_bytes());
        }
        for (key, value) in extra {
            text.extend_from_slice(format!("{key}/{value}/").as_bytes());
        }
        text.extend_from_slice("$BYTEORD/1,2,3,4/".as_bytes());
        text.extend_from_slice("$DATATYPE/F/".as_bytes());

        let text_start = FCS_HEADER_LEN;
        let text_end = text_start + text.len();
        let data_start = text_end;
        let data_end = data_start;
        let analysis_start = data_end + 1;
        let analysis_end = analysis_start + 1;
        fn ptr(n: usize) -> [u8; 8] {
            let s = format!("{n:>8}");
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(s.as_bytes());
            bytes
        }

        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"FCS3.0    ");
        buffer.extend_from_slice(&ptr(text_start));
        buffer.extend_from_slice(&ptr(text_end));
        buffer.extend_from_slice(&ptr(data_start));
        buffer.extend_from_slice(&ptr(data_end));
        buffer.extend_from_slice(&ptr(analysis_start));
        buffer.extend_from_slice(&ptr(analysis_end));
        buffer.extend_from_slice(&text);
        for _ in 0..events {
            for _ in 0..params {
                buffer.extend_from_slice(&0.0_f32.to_le_bytes());
            }
        }
        buffer
    }

    fn builder_fcs() -> FcsFile {
        let mut builder = FcsBuilder::new("FCS3.0");
        builder.add_keyword("$CYT", "Test Cytometer");
        builder.add_keyword("$DATE", "01-JAN-2026");
        builder.add_keyword("$FIL", "built.fcs");
        for (index, name) in ["FSC", "FL1"].iter().enumerate() {
            builder.add_parameter(Parameter {
                name: (*name).to_string(),
                description: format!("Channel {index}"),
                range: 1024.0,
                exponent: 0.0,
                min: 0.0,
                max: 1024.0,
            });
        }
        builder.add_events([0.0, 1.0, 2.0, 3.0]);
        builder.build().expect("build fcs")
    }

    #[test]
    fn fcs_builder_emits_parseable_fcs3() {
        let fcs = builder_fcs();
        assert_eq!(fcs.version, "FCS3.0");
        assert_eq!(fcs.event_count, 2);
        assert_eq!(fcs.parameters.len(), 2);
        let bytes = fcs_to_bytes(&fcs).expect("serialize");
        assert_eq!(&bytes[..6], b"FCS3.0");
        let parsed = FcsParser::parse_bytes(&bytes).expect("parse built fcs");
        assert_eq!(parsed.version, "FCS3.0");
        assert_eq!(parsed.event_count, 2);
        assert_eq!(
            parsed.keywords.get("$CYT").map(String::as_str),
            Some("Test Cytometer")
        );
        assert_eq!(
            parsed.keywords.get("$DATE").map(String::as_str),
            Some("01-JAN-2026")
        );
        assert_eq!(
            parsed.keywords.get("$FIL").map(String::as_str),
            Some("built.fcs")
        );
        assert_eq!(parsed.keywords.get("$TOT").map(String::as_str), Some("2"));
        assert_eq!(parsed.keywords.get("$PAR").map(String::as_str), Some("2"));
        assert_eq!(parsed.data.row(0)[0], 0.0);
        assert_eq!(parsed.data.row(1)[1], 3.0);
    }

    #[test]
    fn fcs_writer_round_trips_to_disk() {
        let fcs = builder_fcs();
        let directory =
            std::env::temp_dir().join(format!("flowers-merge-{}-fcs", std::process::id()));
        let path = directory.with_extension("fcs");
        write_fcs(&fcs, &path).expect("write fcs");
        let bytes = fs::read(&path).expect("read fcs");
        let parsed = FcsParser::parse_bytes(&bytes).expect("parse file");
        assert_eq!(parsed.event_count, fcs.event_count);
        assert_eq!(parsed.parameters.len(), fcs.parameters.len());
        for event in 0..fcs.event_count {
            for channel in 0..fcs.parameters.len() {
                assert_eq!(
                    parsed.data.row(event)[channel],
                    fcs.data.row(event)[channel]
                );
            }
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn merge_fcs_aligns_by_name_and_concatenates_events() {
        let first =
            FcsParser::parse_bytes(&build_fcs(2, 2, &[("$FIL", "a.fcs")])).expect("parse a");
        let second =
            FcsParser::parse_bytes(&build_fcs(3, 2, &[("$FIL", "b.fcs")])).expect("parse b");

        let merged = merge_fcs(&[first, second], &MergeOptions::new()).expect("merge");
        assert_eq!(merged.event_count, 5);
        assert_eq!(merged.parameters.len(), 2);
        assert_eq!(merged.keywords.get("$TOT").map(String::as_str), Some("5"));

        let merged = merge_fcs(
            &[
                FcsParser::parse_bytes(&build_fcs(2, 2, &[("$FIL", "a.fcs")])).expect("parse a"),
                FcsParser::parse_bytes(&build_fcs(3, 3, &[("$FIL", "b.fcs")])).expect("parse b"),
            ],
            &MergeOptions::new().with_align_by_name(true),
        )
        .expect("merge union");
        assert_eq!(merged.parameters.len(), 3);
        assert_eq!(merged.event_count, 5);
        assert_eq!(merged.data.nrows(), 5);
    }

    #[test]
    fn merge_fcs_positional_merge_uses_first_layout() {
        let first =
            FcsParser::parse_bytes(&build_fcs(2, 2, &[("$FIL", "a.fcs")])).expect("parse a");
        let second =
            FcsParser::parse_bytes(&build_fcs(2, 2, &[("$FIL", "b.fcs")])).expect("parse b");
        let merged = merge_fcs(&[first, second], &MergeOptions::new()).expect("merge");
        assert_eq!(merged.event_count, 4);
        assert_eq!(merged.parameters.len(), 2);
        assert_eq!(merged.data.nrows(), 4);
    }

    #[test]
    fn merge_workspace_samples_creates_merged_sample() {
        let mut workspace = Workspace::new();
        workspace.add_sample(
            "a",
            FcsParser::parse_bytes(&build_fcs(2, 1, &[])).expect("parse a"),
        );
        workspace.add_sample(
            "b",
            FcsParser::parse_bytes(&build_fcs(3, 1, &[])).expect("parse b"),
        );
        let merged_id =
            merge_workspace_samples(&mut workspace, &["a".to_string(), "b".to_string()])
                .expect("merge workspace samples");
        assert_eq!(merged_id, "Merged (2)");
        let merged = workspace.samples.get(&merged_id).expect("merged sample");
        assert_eq!(merged.fcs_file.event_count, 5);
        assert_eq!(
            merged.fcs_file.keywords.get("$TOT").map(String::as_str),
            Some("5")
        );
    }

    #[test]
    fn population_rows_export_gated_populations() {
        let mut workspace = Workspace::new();
        workspace.add_sample(
            "s1",
            FcsParser::parse_bytes(&build_fcs(100, 2, &[])).expect("parse s1"),
        );
        workspace
            .add_root_population("s1", "Live", vec![1, 2, 3])
            .expect("root pop");
        workspace
            .samples
            .get_mut("s1")
            .expect("sample s1")
            .gates
            .push(crate::gating::GatedPopulation::new(
                "Live",
                vec![0, 4, 8],
                "rect[0,1] x∈[0,10]",
            ));
        let rows = population_rows(&workspace);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sample, "s1");
        assert_eq!(rows[0].population, "Live");
        assert_eq!(rows[0].count, 3);
        assert_eq!(rows[0].total, 100);
        assert_eq!(rows[0].parent, None);
        let csv = crate::export::populations_csv(&rows);
        assert!(csv.contains(
            "Sample,Population,Parent,Count,Total,PercentOfTotal,PercentOfParent,GateSummary"
        ));
        assert!(csv.contains("s1,Live,,3,100,3.0000,,"));
        assert!(csv.contains("rect[0,1]"));
    }

    #[test]
    fn population_rows_for_missing_sample_is_empty() {
        let workspace = Workspace::new();
        assert!(population_rows_for_sample(&workspace, "nope").is_empty());
    }
}
