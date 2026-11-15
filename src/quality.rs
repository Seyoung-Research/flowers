//! Sample quality control and FCS scanning (Phase 9, W4).
//!
//! This module provides:
//! - per-sample quality-control reports (event counts, flow rate, saturation,
//!   channel CVs and structured issue codes),
//! - recursive FCS scanning with keyword extraction and duplicate detection,
//! - keyword / event / text search over scan results,
//! - missing-file reconnection against a scan report,
//! - scan-result export to CSV, Excel (XLSX), SQL dumps and workspaces.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::fcs::{FcsFile, FcsParser};
use crate::groups::{GroupCriterion, keyword_value};
use crate::project::Project;
use crate::statistics::{SampleQualityReport, compute_sample_quality};
use crate::workspace::Workspace;

/// Errors produced by quality assessment, scanning and export.
#[derive(Debug, Error)]
pub enum QualityError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("FCS error: {0}")]
    Fcs(#[from] crate::fcs::FcsError),
    #[error("export error: {0}")]
    Export(String),
}

/// Severity of a quality-control issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QualitySeverity {
    Info,
    Warning,
    Error,
}

impl QualitySeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// One structured quality-control finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityIssue {
    /// Stable machine-readable issue code (for example `low_event_count`).
    pub code: String,
    pub severity: QualitySeverity,
    pub message: String,
}

impl QualityIssue {
    pub fn new(
        code: impl Into<String>,
        severity: QualitySeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            message: message.into(),
        }
    }

    pub fn info(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, QualitySeverity::Info, message)
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, QualitySeverity::Warning, message)
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, QualitySeverity::Error, message)
    }
}

/// Thresholds used when assessing a sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityThresholds {
    /// Samples below this event count are flagged.
    pub min_events: usize,
    /// Samples above this event count are flagged.
    pub max_events: usize,
    /// Fraction of events allowed at or above the detector range.
    pub max_saturation_fraction: f64,
    /// Highest allowed channel CV; `0.0` disables the check.
    pub max_channel_cv: f64,
    /// Explicit saturation value; derived from the largest `$PnR` when `None`.
    pub saturation_value: Option<f64>,
    /// Keywords that should be present in a complete FCS file.
    pub required_keywords: Vec<String>,
}

impl Default for QualityThresholds {
    fn default() -> Self {
        Self {
            min_events: 1_000,
            max_events: 10_000_000,
            max_saturation_fraction: 0.5,
            max_channel_cv: 0.0,
            saturation_value: None,
            required_keywords: vec![
                "$TOT".to_string(),
                "$PAR".to_string(),
                "$CYT".to_string(),
                "$DATE".to_string(),
            ],
        }
    }
}

impl QualityThresholds {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_min_events(mut self, events: usize) -> Self {
        self.min_events = events;
        self
    }

    pub fn with_required_keywords<I, S>(mut self, keywords: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.required_keywords = keywords.into_iter().map(Into::into).collect();
        self
    }
}

/// Quality-control report for one sample.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleQualityControl {
    pub sample_id: String,
    pub path: Option<PathBuf>,
    pub fcs_version: String,
    pub event_count: usize,
    pub parameter_count: usize,
    pub duration: f64,
    pub flow_rate: f64,
    pub saturation_fraction: f64,
    pub channel_cvs: Vec<f64>,
    pub issues: Vec<QualityIssue>,
    /// Underlying statistical quality metrics (A9).
    pub metrics: SampleQualityReport,
}

impl SampleQualityControl {
    /// Assess an in-memory FCS file.
    pub fn assess(
        sample_id: impl Into<String>,
        fcs: &FcsFile,
        thresholds: &QualityThresholds,
    ) -> Self {
        Self::assess_impl(sample_id.into(), None, fcs, thresholds)
    }

    /// Assess an in-memory FCS file and record the source path.
    pub fn assess_with_path(
        sample_id: impl Into<String>,
        path: impl Into<PathBuf>,
        fcs: &FcsFile,
        thresholds: &QualityThresholds,
    ) -> Self {
        Self::assess_impl(sample_id.into(), Some(path.into()), fcs, thresholds)
    }

    /// Read and assess an FCS file from disk.
    pub fn assess_file(
        sample_id: impl Into<String>,
        path: impl AsRef<Path>,
        thresholds: &QualityThresholds,
    ) -> Result<Self, QualityError> {
        let path = path.as_ref();
        let bytes = fs::read(path)?;
        let fcs = FcsParser::parse_bytes(&bytes)?;
        Ok(Self::assess_with_path(sample_id, path, &fcs, thresholds))
    }

    fn assess_impl(
        sample_id: String,
        path: Option<PathBuf>,
        fcs: &FcsFile,
        thresholds: &QualityThresholds,
    ) -> Self {
        let parameter_count = fcs.parameters.len();
        let n_params = fcs.data.ncols().max(1);
        let values: Vec<f64> = fcs.data.iter().map(|v| *v as f64).collect();
        let time_channel = fcs.parameters.iter().position(|parameter| {
            let name = parameter.name.to_ascii_lowercase();
            name.contains("time") || parameter.description.to_ascii_lowercase().contains("time")
        });
        let saturation_value = thresholds.saturation_value.unwrap_or_else(|| {
            let max_range = fcs
                .parameters
                .iter()
                .map(|parameter| parameter.range as f64)
                .fold(0.0_f64, f64::max);
            if max_range > 0.0 {
                max_range * 0.99
            } else {
                f64::INFINITY
            }
        });
        let metrics = compute_sample_quality(&values, n_params, time_channel, saturation_value);

        let mut issues = Vec::new();
        for keyword in &thresholds.required_keywords {
            if keyword_value(&fcs.keywords, keyword).is_none() {
                issues.push(QualityIssue::warning(
                    "missing_keyword",
                    format!("missing required keyword {keyword}"),
                ));
            }
        }

        if fcs.event_count == 0 {
            issues.push(QualityIssue::error("no_events", "file contains no events"));
        } else if fcs.event_count < thresholds.min_events {
            issues.push(QualityIssue::warning(
                "low_event_count",
                format!(
                    "only {} events (minimum {})",
                    fcs.event_count, thresholds.min_events
                ),
            ));
        }
        if fcs.event_count > thresholds.max_events {
            issues.push(QualityIssue::warning(
                "high_event_count",
                format!(
                    "{} events exceeds maximum {}",
                    fcs.event_count, thresholds.max_events
                ),
            ));
        }
        if parameter_count == 0 {
            issues.push(QualityIssue::error(
                "no_parameters",
                "file defines no parameters",
            ));
        }
        if fcs.data.nrows() != fcs.event_count {
            issues.push(QualityIssue::error(
                "event_data_mismatch",
                format!(
                    "header reports {} events but data contains {} rows",
                    fcs.event_count,
                    fcs.data.nrows()
                ),
            ));
        }
        if fcs.data.ncols() != parameter_count {
            issues.push(QualityIssue::error(
                "parameter_data_mismatch",
                format!(
                    "{} parameters defined but data contains {} columns",
                    parameter_count,
                    fcs.data.ncols()
                ),
            ));
        }
        if let Some(total) =
            keyword_value(&fcs.keywords, "$TOT").and_then(|value| value.parse::<usize>().ok())
            && total != fcs.event_count
        {
            issues.push(QualityIssue::error(
                "event_count_mismatch",
                format!("$TOT is {total} but parser read {} events", fcs.event_count),
            ));
        }
        if let Some(parameters) =
            keyword_value(&fcs.keywords, "$PAR").and_then(|value| value.parse::<usize>().ok())
            && parameters != parameter_count
        {
            issues.push(QualityIssue::error(
                "parameter_count_mismatch",
                format!(
                    "$PAR is {parameters} but {parameter_count} parameter definitions were read"
                ),
            ));
        }
        if let Some(parameter) = fcs
            .parameters
            .iter()
            .find(|parameter| parameter.range <= 0.0)
        {
            issues.push(QualityIssue::warning(
                "invalid_parameter_range",
                format!("parameter {} has a non-positive range", parameter.name),
            ));
        }
        if metrics.saturation_fraction > thresholds.max_saturation_fraction {
            issues.push(QualityIssue::warning(
                "high_saturation",
                format!(
                    "{:.1}% of events are at or above the detector range",
                    metrics.saturation_fraction * 100.0
                ),
            ));
        }
        if thresholds.max_channel_cv > 0.0
            && let Some((index, cv)) = metrics
                .channel_cvs
                .iter()
                .enumerate()
                .filter(|(_, cv)| cv.is_finite())
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            && *cv > thresholds.max_channel_cv
        {
            let name = fcs
                .parameters
                .get(index)
                .map(|parameter| parameter.name.as_str())
                .unwrap_or("channel");
            issues.push(QualityIssue::warning(
                "high_channel_cv",
                format!(
                    "{name} CV {:.2} exceeds {:.2}",
                    cv, thresholds.max_channel_cv
                ),
            ));
        }
        if time_channel.is_some() {
            if metrics.duration <= 0.0 {
                issues.push(QualityIssue::warning(
                    "zero_duration",
                    "time channel reports no acquisition duration",
                ));
            } else if metrics.flow_rate <= 0.0 {
                issues.push(QualityIssue::warning(
                    "zero_flow_rate",
                    "computed acquisition flow rate is zero",
                ));
            }
        }
        let non_finite = values.iter().filter(|value| !value.is_finite()).count();
        if non_finite > 0 {
            issues.push(QualityIssue::error(
                "non_finite_values",
                format!("{non_finite} event values are NaN or infinite"),
            ));
        }
        let negatives = values
            .iter()
            .filter(|value| value.is_finite() && **value < 0.0)
            .count();
        if negatives > 0 {
            issues.push(QualityIssue::warning(
                "negative_values",
                format!("{negatives} event values are below zero"),
            ));
        }

        Self {
            sample_id,
            path,
            fcs_version: fcs.version.clone(),
            event_count: fcs.event_count,
            parameter_count,
            duration: metrics.duration,
            flow_rate: metrics.flow_rate,
            saturation_fraction: metrics.saturation_fraction,
            channel_cvs: metrics.channel_cvs.clone(),
            issues,
            metrics,
        }
    }

    /// `pass`, `warn` or `fail`.
    pub fn status(&self) -> &'static str {
        if self
            .issues
            .iter()
            .any(|issue| issue.severity == QualitySeverity::Error)
        {
            "fail"
        } else if self
            .issues
            .iter()
            .any(|issue| issue.severity == QualitySeverity::Warning)
        {
            "warn"
        } else {
            "pass"
        }
    }

    /// True when no error-level issue was found (warnings still pass).
    pub fn passed(&self) -> bool {
        self.status() != "fail"
    }

    pub fn has_warnings(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == QualitySeverity::Warning)
    }

    pub fn highest_severity(&self) -> Option<QualitySeverity> {
        self.issues.iter().map(|issue| issue.severity).max()
    }

    pub fn issue_codes(&self) -> Vec<&str> {
        self.issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect()
    }

    pub fn has_issue(&self, code: &str) -> bool {
        self.issues.iter().any(|issue| issue.code == code)
    }

    pub fn max_channel_cv(&self) -> f64 {
        self.channel_cvs
            .iter()
            .copied()
            .filter(|cv| cv.is_finite())
            .fold(0.0_f64, f64::max)
    }
}

/// Aggregate quality-control results for several samples.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QualityControlReport {
    pub reports: Vec<SampleQualityControl>,
}

/// Counts of passing, warning and failing samples.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualitySummary {
    pub total: usize,
    pub passed: usize,
    pub warned: usize,
    pub failed: usize,
}

impl QualitySummary {
    /// Fraction of samples with no findings at all.
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.passed as f64 / self.total as f64
        }
    }
}

impl QualityControlReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, report: SampleQualityControl) {
        self.reports.push(report);
    }

    pub fn len(&self) -> usize {
        self.reports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.reports.is_empty()
    }

    pub fn summary(&self) -> QualitySummary {
        let mut summary = QualitySummary {
            total: self.reports.len(),
            ..QualitySummary::default()
        };
        for report in &self.reports {
            match report.status() {
                "pass" => summary.passed += 1,
                "warn" => summary.warned += 1,
                _ => summary.failed += 1,
            }
        }
        summary
    }

    pub fn failures(&self) -> impl Iterator<Item = &SampleQualityControl> {
        self.reports.iter().filter(|report| !report.passed())
    }

    pub fn warnings(&self) -> impl Iterator<Item = &SampleQualityControl> {
        self.reports
            .iter()
            .filter(|report| report.status() == "warn")
    }

    pub fn to_csv(&self) -> String {
        let mut out = String::from(
            "Sample,Path,Status,Version,Events,Parameters,Duration,FlowRate,Saturation,MaxCV,Issues\n",
        );
        for report in &self.reports {
            let issues = report
                .issues
                .iter()
                .map(|issue| format!("{}:{}", issue.severity.label(), issue.code))
                .collect::<Vec<_>>()
                .join(";");
            let row = [
                report.sample_id.clone(),
                report
                    .path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
                report.status().to_string(),
                report.fcs_version.clone(),
                report.event_count.to_string(),
                report.parameter_count.to_string(),
                format!("{:.3}", report.duration),
                format!("{:.3}", report.flow_rate),
                format!("{:.4}", report.saturation_fraction),
                format!("{:.4}", report.max_channel_cv()),
                issues,
            ];
            out.push_str(
                &row.iter()
                    .map(|value| csv_escape(value))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            out.push('\n');
        }
        out
    }

    pub fn write_csv(&self, path: impl AsRef<Path>) -> Result<(), QualityError> {
        fs::write(path, self.to_csv())?;
        Ok(())
    }
}

/// Assess every sample in a workspace.
pub fn assess_workspace(
    workspace: &Workspace,
    thresholds: &QualityThresholds,
) -> QualityControlReport {
    let mut report = QualityControlReport::new();
    for id in &workspace.sample_order {
        if let Some(sample) = workspace.samples.get(id) {
            report.add(SampleQualityControl::assess(
                id.clone(),
                &sample.fcs_file,
                thresholds,
            ));
        }
    }
    report
}

/// Options controlling directory scans.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanOptions {
    pub recursive: bool,
    pub follow_symlinks: bool,
    /// Extensions to include (with or without a leading dot). Empty means all.
    pub extensions: Vec<String>,
    /// Optional cap on the number of files scanned.
    pub max_files: Option<usize>,
    /// Compute a quality-control report for each parsed file.
    pub compute_quality: bool,
    pub thresholds: QualityThresholds,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            follow_symlinks: false,
            extensions: vec!["fcs".to_string()],
            max_files: None,
            compute_quality: true,
            thresholds: QualityThresholds::default(),
        }
    }
}

impl ScanOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_recursive(mut self, recursive: bool) -> Self {
        self.recursive = recursive;
        self
    }

    pub fn with_max_files(mut self, max_files: usize) -> Self {
        self.max_files = Some(max_files);
        self
    }

    pub fn without_quality(mut self) -> Self {
        self.compute_quality = false;
        self
    }
}

/// One file discovered by a scan.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// FNV-1a hash of the complete file contents.
    pub content_hash: u64,
    pub fcs_version: Option<String>,
    pub event_count: Option<usize>,
    pub parameter_count: Option<usize>,
    pub keywords: HashMap<String, String>,
    pub parse_error: Option<String>,
    pub quality: Option<SampleQualityControl>,
}

impl ScannedFile {
    /// Case-insensitive keyword lookup.
    pub fn keyword(&self, keyword: &str) -> Option<&str> {
        keyword_value(&self.keywords, keyword)
    }

    /// `$FIL` when present, otherwise the on-disk file name.
    pub fn display_name(&self) -> &str {
        self.keyword("$FIL").unwrap_or(&self.file_name)
    }

    pub fn status(&self) -> &'static str {
        if self.parse_error.is_some() {
            return "error";
        }
        match self.quality.as_ref().map(SampleQualityControl::status) {
            Some(status) => status,
            None => "ok",
        }
    }

    pub fn quality_failed(&self) -> bool {
        self.quality
            .as_ref()
            .is_some_and(|quality| !quality.passed())
    }

    /// Test this entry against a scan query.
    pub fn matches(&self, query: &ScanQuery) -> bool {
        if !query.include_parse_errors && self.parse_error.is_some() {
            return false;
        }
        if query.failing_quality_only && !self.quality_failed() {
            return false;
        }
        if !query.text.trim().is_empty() {
            let needle = query.text.to_ascii_lowercase();
            let mut haystack = String::new();
            haystack.push_str(&self.path.to_string_lossy());
            haystack.push('\n');
            haystack.push_str(&self.file_name);
            for (key, value) in &self.keywords {
                haystack.push('\n');
                haystack.push_str(key);
                haystack.push('\n');
                haystack.push_str(value);
            }
            if !haystack.to_ascii_lowercase().contains(&needle) {
                return false;
            }
        }
        if !query
            .criteria
            .iter()
            .all(|criterion| criterion.matches(&self.keywords))
        {
            return false;
        }
        if query.min_events.is_some() || query.max_events.is_some() {
            let Some(count) = self.event_count else {
                return false;
            };
            if let Some(minimum) = query.min_events
                && count < minimum
            {
                return false;
            }
            if let Some(maximum) = query.max_events
                && count > maximum
            {
                return false;
            }
        }
        if let Some(prefix) = &query.version_prefix {
            let Some(version) = &self.fcs_version else {
                return false;
            };
            if !version
                .to_ascii_lowercase()
                .starts_with(&prefix.to_ascii_lowercase())
            {
                return false;
            }
        }
        true
    }
}

/// Search criteria over a scan report.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanQuery {
    /// Case-insensitive substring matched against path, file name and keywords.
    pub text: String,
    /// Additional keyword criteria (all must match).
    pub criteria: Vec<GroupCriterion>,
    pub min_events: Option<usize>,
    pub max_events: Option<usize>,
    /// Required FCS version prefix, for example `FCS3`.
    pub version_prefix: Option<String>,
    pub include_parse_errors: bool,
    pub failing_quality_only: bool,
}

impl Default for ScanQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            criteria: Vec::new(),
            min_events: None,
            max_events: None,
            version_prefix: None,
            include_parse_errors: true,
            failing_quality_only: false,
        }
    }
}

impl ScanQuery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    pub fn with_criterion(mut self, criterion: GroupCriterion) -> Self {
        self.criteria.push(criterion);
        self
    }

    pub fn with_event_range(mut self, min: usize, max: usize) -> Self {
        self.min_events = Some(min);
        self.max_events = Some(max);
        self
    }

    pub fn with_version(mut self, prefix: impl Into<String>) -> Self {
        self.version_prefix = Some(prefix.into());
        self
    }

    pub fn failing_quality_only(mut self) -> Self {
        self.failing_quality_only = true;
        self
    }
}

/// How two scanned files are considered duplicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateKind {
    /// Byte-for-byte identical content.
    IdenticalContent,
    /// Same file name and size but different content.
    SameNameAndSize,
}

impl DuplicateKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::IdenticalContent => "identical content",
            Self::SameNameAndSize => "same name and size",
        }
    }
}

/// A set of files that look like duplicates.
#[derive(Debug, Clone, PartialEq)]
pub struct DuplicateGroup {
    pub kind: DuplicateKind,
    pub signature: String,
    pub paths: Vec<PathBuf>,
}

impl DuplicateGroup {
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// Result of scanning one or more directories.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanReport {
    pub root: PathBuf,
    pub files: Vec<ScannedFile>,
    pub duplicates: Vec<DuplicateGroup>,
}

impl ScanReport {
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn parse_errors(&self) -> impl Iterator<Item = &ScannedFile> {
        self.files.iter().filter(|file| file.parse_error.is_some())
    }

    pub fn search(&self, query: &ScanQuery) -> Vec<&ScannedFile> {
        self.files
            .iter()
            .filter(|file| file.matches(query))
            .collect()
    }

    /// Quality reports for every scanned file that was parsed.
    pub fn quality_report(&self) -> QualityControlReport {
        QualityControlReport {
            reports: self
                .files
                .iter()
                .filter_map(|file| file.quality.clone())
                .collect(),
        }
    }

    pub fn duplicate_for(&self, path: &Path) -> Option<&DuplicateGroup> {
        self.duplicates
            .iter()
            .find(|group| group.paths.iter().any(|candidate| candidate == path))
    }

    /// Header plus one row per scanned file, used by the CSV/XLSX writers.
    pub fn table_rows(&self) -> (Vec<String>, Vec<Vec<String>>) {
        let header = vec![
            "File".to_string(),
            "Path".to_string(),
            "Status".to_string(),
            "Version".to_string(),
            "Events".to_string(),
            "Parameters".to_string(),
            "Size".to_string(),
            "Hash".to_string(),
            "Duplicate".to_string(),
            "Issues".to_string(),
        ];
        let rows = self
            .files
            .iter()
            .map(|file| {
                let duplicate = self
                    .duplicate_for(&file.path)
                    .map(|group| format!("{}:{}", group.kind.label(), group.signature))
                    .unwrap_or_default();
                let mut issues = Vec::new();
                if let Some(error) = &file.parse_error {
                    issues.push(error.clone());
                }
                if let Some(quality) = &file.quality {
                    issues.extend(
                        quality
                            .issues
                            .iter()
                            .map(|issue| format!("{}:{}", issue.severity.label(), issue.code)),
                    );
                }
                vec![
                    file.file_name.clone(),
                    file.path.display().to_string(),
                    file.status().to_string(),
                    file.fcs_version.clone().unwrap_or_default(),
                    file.event_count
                        .map(|count| count.to_string())
                        .unwrap_or_default(),
                    file.parameter_count
                        .map(|count| count.to_string())
                        .unwrap_or_default(),
                    file.size.to_string(),
                    format!("{:016x}", file.content_hash),
                    duplicate,
                    issues.join(";"),
                ]
            })
            .collect();
        (header, rows)
    }

    pub fn to_csv(&self) -> String {
        let (header, rows) = self.table_rows();
        csv_document(&header, &rows)
    }

    pub fn search_to_csv(&self, query: &ScanQuery) -> String {
        let matches: HashSet<&Path> = self
            .search(query)
            .into_iter()
            .map(|file| file.path.as_path())
            .collect();
        let (header, rows) = self.table_rows();
        let filtered = self
            .files
            .iter()
            .zip(rows)
            .filter(|(file, _)| matches.contains(file.path.as_path()))
            .map(|(_, row)| row)
            .collect::<Vec<_>>();
        csv_document(&header, &filtered)
    }

    pub fn write_csv(&self, path: impl AsRef<Path>) -> Result<(), QualityError> {
        fs::write(path, self.to_csv())?;
        Ok(())
    }

    /// Excel workbook with a `Scan` sheet and a `Quality` sheet.
    pub fn to_xlsx(&self) -> Result<Vec<u8>, QualityError> {
        let (header, rows) = self.table_rows();
        let mut workbook = rust_xlsxwriter::Workbook::new();
        {
            let sheet = workbook.add_worksheet();
            sheet
                .set_name("Scan")
                .map_err(|error| QualityError::Export(error.to_string()))?;
            sheet
                .write_row(0, 0, header)
                .map_err(|error| QualityError::Export(error.to_string()))?;
            for (index, row) in rows.into_iter().enumerate() {
                sheet
                    .write_row(index as u32 + 1, 0, row)
                    .map_err(|error| QualityError::Export(error.to_string()))?;
            }
        }
        {
            let sheet = workbook.add_worksheet();
            sheet
                .set_name("Quality")
                .map_err(|error| QualityError::Export(error.to_string()))?;
            let quality_header = vec![
                "Sample".to_string(),
                "Events".to_string(),
                "Duration".to_string(),
                "FlowRate".to_string(),
                "Saturation".to_string(),
                "MaxCV".to_string(),
                "Status".to_string(),
                "Issues".to_string(),
            ];
            sheet
                .write_row(0, 0, quality_header)
                .map_err(|error| QualityError::Export(error.to_string()))?;
            for (index, quality) in self
                .files
                .iter()
                .filter_map(|file| file.quality.as_ref())
                .enumerate()
            {
                let row = vec![
                    quality.sample_id.clone(),
                    quality.event_count.to_string(),
                    format!("{:.3}", quality.duration),
                    format!("{:.3}", quality.flow_rate),
                    format!("{:.4}", quality.saturation_fraction),
                    format!("{:.4}", quality.max_channel_cv()),
                    quality.status().to_string(),
                    quality.issue_codes().join(";"),
                ];
                sheet
                    .write_row(index as u32 + 1, 0, row)
                    .map_err(|error| QualityError::Export(error.to_string()))?;
            }
        }
        workbook
            .save_to_buffer()
            .map_err(|error| QualityError::Export(error.to_string()))
    }

    pub fn write_xlsx(&self, path: impl AsRef<Path>) -> Result<(), QualityError> {
        fs::write(path, self.to_xlsx()?)?;
        Ok(())
    }

    /// SQL dump (CREATE TABLE plus one INSERT per file) suitable for SQLite or
    /// PostgreSQL.
    pub fn to_sql(&self, table: &str) -> String {
        let table = sql_identifier(table);
        let mut out = format!(
            "CREATE TABLE IF NOT EXISTS {table} (\n  file_name TEXT,\n  path TEXT,\n  status TEXT,\n  version TEXT,\n  events INTEGER,\n  parameters INTEGER,\n  size_bytes INTEGER,\n  content_hash TEXT,\n  duplicate TEXT,\n  issues TEXT\n);\n"
        );
        for file in &self.files {
            let duplicate = self
                .duplicate_for(&file.path)
                .map(|group| format!("{}:{}", group.kind.label(), group.signature))
                .unwrap_or_default();
            let mut issues = Vec::new();
            if let Some(error) = &file.parse_error {
                issues.push(error.clone());
            }
            if let Some(quality) = &file.quality {
                issues.extend(
                    quality
                        .issues
                        .iter()
                        .map(|issue| format!("{}:{}", issue.severity.label(), issue.code)),
                );
            }
            out.push_str(&format!(
                "INSERT INTO {table} (file_name, path, status, version, events, parameters, size_bytes, content_hash, duplicate, issues) VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {});\n",
                sql_text(&file.file_name),
                sql_text(&file.path.display().to_string()),
                sql_text(file.status()),
                sql_text(file.fcs_version.as_deref().unwrap_or("")),
                sql_integer(file.event_count),
                sql_integer(file.parameter_count),
                file.size,
                sql_text(&format!("{:016x}", file.content_hash)),
                sql_text(&duplicate),
                sql_text(&issues.join(";")),
            ));
        }
        out
    }

    pub fn write_sql(&self, path: impl AsRef<Path>, table: &str) -> Result<(), QualityError> {
        fs::write(path, self.to_sql(table))?;
        Ok(())
    }

    /// Add every successfully scanned file to a workspace.
    ///
    /// Sample ids are derived from `$FIL` (or the file name) and made unique
    /// with a numeric suffix when a workspace already contains that id.
    pub fn add_to_workspace(&self, workspace: &mut Workspace) -> Result<Vec<String>, QualityError> {
        let mut added = Vec::new();
        for file in &self.files {
            if file.parse_error.is_some() {
                continue;
            }
            let bytes = fs::read(&file.path)?;
            let fcs = FcsParser::parse_bytes(&bytes)?;
            let base = fcs
                .keywords
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("$FIL"))
                .map(|(_, value)| value.as_str())
                .unwrap_or(&file.file_name);
            let base = Path::new(base)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&file.file_name);
            let base = if base.trim().is_empty() {
                file.file_name.as_str()
            } else {
                base
            };
            let id = unique_sample_id(workspace, base);
            workspace.add_sample(&id, fcs);
            added.push(id);
        }
        Ok(added)
    }

    /// Build a new workspace from every successfully scanned file.
    pub fn to_workspace(&self) -> Result<Workspace, QualityError> {
        let mut workspace = Workspace::new();
        self.add_to_workspace(&mut workspace)?;
        Ok(workspace)
    }
}

fn unique_sample_id(workspace: &Workspace, base: &str) -> String {
    if !workspace.samples.contains_key(base) {
        return base.to_string();
    }
    for suffix in 2.. {
        let candidate = format!("{base} ({suffix})");
        if !workspace.samples.contains_key(&candidate) {
            return candidate;
        }
    }
    unreachable!("an unused sample id always exists")
}

/// Scanner with reusable options.
#[derive(Debug, Clone)]
pub struct FcsScanner {
    pub options: ScanOptions,
}

impl Default for FcsScanner {
    fn default() -> Self {
        Self::new(ScanOptions::default())
    }
}

impl FcsScanner {
    pub fn new(options: ScanOptions) -> Self {
        Self { options }
    }

    pub fn scan(&self, root: impl AsRef<Path>) -> Result<ScanReport, QualityError> {
        scan_directory(root, &self.options)
    }

    pub fn scan_files(&self, paths: &[PathBuf]) -> Result<ScanReport, QualityError> {
        scan_fcs_files(paths, &self.options)
    }
}

/// Recursively scan `root` for FCS files.
pub fn scan_directory(
    root: impl AsRef<Path>,
    options: &ScanOptions,
) -> Result<ScanReport, QualityError> {
    let root = root.as_ref();
    let metadata = fs::metadata(root)?;
    let (report_root, files) = if metadata.is_file() {
        let paths = if matches_extension(root, options) {
            vec![root.to_path_buf()]
        } else {
            Vec::new()
        };
        (
            root.parent().unwrap_or(Path::new("")).to_path_buf(),
            scan_fcs_files(&paths, options)?,
        )
    } else {
        let mut paths = Vec::new();
        collect_fcs_paths(root, options, &mut paths)?;
        (root.to_path_buf(), scan_fcs_files(&paths, options)?)
    };
    let mut report = files;
    report.root = report_root;
    Ok(report)
}

/// Scan an explicit list of files.
pub fn scan_fcs_files(
    paths: &[PathBuf],
    options: &ScanOptions,
) -> Result<ScanReport, QualityError> {
    let mut unique: Vec<PathBuf> = paths
        .iter()
        .filter(|path| path.is_file() && matches_extension(path, options))
        .cloned()
        .collect();
    unique.sort();
    unique.dedup();
    if let Some(maximum) = options.max_files {
        unique.truncate(maximum);
    }
    let mut files = Vec::with_capacity(unique.len());
    for path in &unique {
        files.push(scan_fcs_file(path, options)?);
    }
    let duplicates = detect_duplicates(&files);
    Ok(ScanReport {
        root: unique
            .first()
            .and_then(|path| path.parent())
            .unwrap_or(Path::new(""))
            .to_path_buf(),
        files,
        duplicates,
    })
}

/// Scan a single FCS file, capturing parse errors instead of returning them.
pub fn scan_fcs_file(
    path: impl AsRef<Path>,
    options: &ScanOptions,
) -> Result<ScannedFile, QualityError> {
    let path = path.as_ref();
    let metadata = fs::metadata(path)?;
    let bytes = fs::read(path)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    let content_hash = hash_bytes(&bytes);
    let mut scanned = ScannedFile {
        path: path.to_path_buf(),
        file_name: file_name.clone(),
        size: metadata.len(),
        modified: metadata.modified().ok(),
        content_hash,
        fcs_version: None,
        event_count: None,
        parameter_count: None,
        keywords: HashMap::new(),
        parse_error: None,
        quality: None,
    };
    match FcsParser::parse_bytes(&bytes) {
        Ok(fcs) => {
            scanned.fcs_version = Some(fcs.version.clone());
            scanned.event_count = Some(fcs.event_count);
            scanned.parameter_count = Some(fcs.parameters.len());
            scanned.keywords = fcs.keywords.clone();
            if options.compute_quality {
                let sample_id = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or(&file_name)
                    .to_string();
                scanned.quality = Some(SampleQualityControl::assess_with_path(
                    sample_id,
                    path,
                    &fcs,
                    &options.thresholds,
                ));
            }
        }
        Err(error) => scanned.parse_error = Some(error.to_string()),
    }
    Ok(scanned)
}

fn collect_fcs_paths(
    directory: &Path,
    options: &ScanOptions,
    paths: &mut Vec<PathBuf>,
) -> Result<(), QualityError> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            if !options.follow_symlinks {
                continue;
            }
            let metadata = fs::metadata(&path)?;
            if metadata.is_dir() {
                if options.recursive {
                    collect_fcs_paths(&path, options, paths)?;
                }
            } else if metadata.is_file() && matches_extension(&path, options) {
                paths.push(path);
            }
        } else if file_type.is_dir() {
            if options.recursive {
                collect_fcs_paths(&path, options, paths)?;
            }
        } else if file_type.is_file() && matches_extension(&path, options) {
            paths.push(path);
        }
    }
    Ok(())
}

fn matches_extension(path: &Path, options: &ScanOptions) -> bool {
    if options.extensions.is_empty() {
        return true;
    }
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    options.extensions.iter().any(|candidate| {
        candidate
            .trim_start_matches('.')
            .eq_ignore_ascii_case(extension)
    })
}

/// FNV-1a hash of complete file contents.
pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Group scanned files that look like duplicates.
pub fn detect_duplicates(files: &[ScannedFile]) -> Vec<DuplicateGroup> {
    let mut groups = Vec::new();

    let mut by_hash: HashMap<u64, Vec<usize>> = HashMap::new();
    for (index, file) in files.iter().enumerate() {
        by_hash.entry(file.content_hash).or_default().push(index);
    }
    for (hash, indices) in by_hash {
        if indices.len() > 1 {
            let mut paths = indices
                .iter()
                .map(|&index| files[index].path.clone())
                .collect::<Vec<_>>();
            paths.sort();
            groups.push(DuplicateGroup {
                kind: DuplicateKind::IdenticalContent,
                signature: format!("{hash:016x}"),
                paths,
            });
        }
    }

    let mut by_name: HashMap<(String, u64), Vec<usize>> = HashMap::new();
    for (index, file) in files.iter().enumerate() {
        by_name
            .entry((file.file_name.to_ascii_lowercase(), file.size))
            .or_default()
            .push(index);
    }
    for ((name, size), indices) in by_name {
        if indices.len() < 2 {
            continue;
        }
        let hashes: HashSet<u64> = indices
            .iter()
            .map(|&index| files[index].content_hash)
            .collect();
        if hashes.len() > 1 {
            let mut paths = indices
                .iter()
                .map(|&index| files[index].path.clone())
                .collect::<Vec<_>>();
            paths.sort();
            groups.push(DuplicateGroup {
                kind: DuplicateKind::SameNameAndSize,
                signature: format!("{name}:{size}"),
                paths,
            });
        }
    }

    groups.sort_by(|a, b| {
        a.kind
            .label()
            .cmp(b.kind.label())
            .then_with(|| a.signature.cmp(&b.signature))
    });
    groups
}

/// A sample file reference used for missing-file reconnection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReference {
    pub sample_id: String,
    pub path: PathBuf,
    /// Optional byte size from a saved workspace.
    pub size: Option<u64>,
    /// Optional content hash from a previous scan.
    pub content_hash: Option<u64>,
    pub keywords: HashMap<String, String>,
}

impl FileReference {
    pub fn new(sample_id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            sample_id: sample_id.into(),
            path: path.into(),
            size: None,
            content_hash: None,
            keywords: HashMap::new(),
        }
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_hash(mut self, hash: u64) -> Self {
        self.content_hash = Some(hash);
        self
    }

    pub fn with_keyword(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.keywords.insert(key.into(), value.into());
        self
    }

    pub fn is_missing(&self) -> bool {
        !self.path.is_file()
    }
}

/// Return the references whose files no longer exist.
pub fn find_missing_files(references: &[FileReference]) -> Vec<FileReference> {
    references
        .iter()
        .filter(|reference| reference.is_missing())
        .cloned()
        .collect()
}

/// Options controlling reconnection.
#[derive(Debug, Clone, PartialEq)]
pub struct ReconnectOptions {
    /// Minimum score required to accept a candidate.
    pub min_score: u32,
    /// Only accept a candidate when no other file has the same best score.
    pub require_unique: bool,
    /// Allow one scanned file to be assigned to several missing references.
    pub allow_reuse: bool,
}

impl Default for ReconnectOptions {
    fn default() -> Self {
        Self {
            min_score: 40,
            require_unique: false,
            allow_reuse: false,
        }
    }
}

impl ReconnectOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_min_score(mut self, score: u32) -> Self {
        self.min_score = score;
        self
    }
}

/// One proposed replacement for a missing file.
#[derive(Debug, Clone, PartialEq)]
pub struct ReconnectionCandidate {
    pub sample_id: String,
    pub missing_path: PathBuf,
    pub candidate_path: PathBuf,
    pub score: u32,
    pub reasons: Vec<String>,
}

impl ReconnectionCandidate {
    /// Normalised confidence in `[0, 1]`.
    pub fn confidence(&self) -> f64 {
        (self.score.min(100) as f64) / 100.0
    }
}

/// Result of matching missing files against a scan report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReconnectionReport {
    pub missing: Vec<FileReference>,
    pub candidates: Vec<ReconnectionCandidate>,
    pub unmatched: Vec<FileReference>,
}

impl ReconnectionReport {
    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    pub fn best_for(&self, sample_id: &str) -> Option<&ReconnectionCandidate> {
        self.candidates
            .iter()
            .find(|candidate| candidate.sample_id == sample_id)
    }

    /// Update `references` in place; returns the number of paths changed.
    pub fn apply(&self, references: &mut [FileReference]) -> usize {
        let mut changed = 0;
        for reference in references.iter_mut() {
            if let Some(candidate) = self
                .candidates
                .iter()
                .find(|candidate| candidate.sample_id == reference.sample_id)
            {
                reference.path = candidate.candidate_path.clone();
                changed += 1;
            }
        }
        changed
    }

    /// Update a saved project's sample paths; returns the number changed.
    pub fn apply_to_project(&self, project: &mut Project) -> usize {
        let mut changed = 0;
        for sample in &mut project.samples {
            if let Some(candidate) = self
                .candidates
                .iter()
                .find(|candidate| candidate.missing_path == Path::new(&sample.path))
            {
                sample.path = candidate.candidate_path.display().to_string();
                changed += 1;
            }
        }
        changed
    }
}

/// Score one scanned file against a missing reference.
pub fn score_reference(reference: &FileReference, candidate: &ScannedFile) -> (u32, Vec<String>) {
    let mut score = 0_u32;
    let mut reasons = Vec::new();

    if let Some(hash) = reference.content_hash
        && hash == candidate.content_hash
    {
        score += 100;
        reasons.push("identical content".to_string());
    }

    let missing_name = file_name_lower(&reference.path);
    let candidate_name = candidate.file_name.to_ascii_lowercase();
    if !missing_name.is_empty() && missing_name == candidate_name {
        score += 60;
        reasons.push("file name".to_string());
    } else if file_stem_lower(&reference.path) == file_stem_lower(Path::new(&candidate.file_name))
        && !file_stem_lower(&reference.path).is_empty()
    {
        score += 40;
        reasons.push("file stem".to_string());
    }

    if let Some(fil) = candidate.keyword("$FIL") {
        let fil_name = Path::new(fil)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(fil)
            .to_ascii_lowercase();
        if !missing_name.is_empty() && fil_name == missing_name {
            score += 55;
            reasons.push("$FIL keyword".to_string());
        } else if file_stem_lower(Path::new(fil)) == file_stem_lower(&reference.path)
            && !file_stem_lower(&reference.path).is_empty()
        {
            score += 30;
            reasons.push("$FIL stem".to_string());
        }
    }

    if let Some(size) = reference.size
        && size == candidate.size
    {
        score += 20;
        reasons.push("file size".to_string());
    }

    if let Some(total) =
        keyword_value(&reference.keywords, "$TOT").and_then(|value| value.parse::<usize>().ok())
        && Some(total) == candidate.event_count
    {
        score += 10;
        reasons.push("$TOT".to_string());
    }
    if let (Some(expected), Some(actual)) = (
        keyword_value(&reference.keywords, "$DATE"),
        candidate.keyword("$DATE"),
    ) && expected == actual
    {
        score += 8;
        reasons.push("$DATE".to_string());
    }
    if let (Some(expected), Some(actual)) = (
        keyword_value(&reference.keywords, "$BTIM"),
        candidate.keyword("$BTIM"),
    ) && expected == actual
    {
        score += 5;
        reasons.push("$BTIM".to_string());
    }

    (score, reasons)
}

/// Match missing references against scanned files.
pub fn plan_reconnection(
    references: &[FileReference],
    files: &[ScannedFile],
    options: &ReconnectOptions,
) -> ReconnectionReport {
    let mut report = ReconnectionReport {
        missing: references.to_vec(),
        ..ReconnectionReport::default()
    };
    let mut used: HashSet<PathBuf> = HashSet::new();

    for reference in references {
        let mut scored = files
            .iter()
            .filter(|file| options.allow_reuse || !used.contains(&file.path))
            .map(|file| {
                let (score, reasons) = score_reference(reference, file);
                (file, score, reasons)
            })
            .filter(|(_, score, _)| *score >= options.min_score)
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.path.cmp(&b.0.path)));

        let unique = !options.require_unique || scored.len() < 2 || scored[0].1 != scored[1].1;
        match scored.first() {
            Some((file, score, reasons)) if unique => {
                report.candidates.push(ReconnectionCandidate {
                    sample_id: reference.sample_id.clone(),
                    missing_path: reference.path.clone(),
                    candidate_path: file.path.clone(),
                    score: *score,
                    reasons: reasons.clone(),
                });
                if !options.allow_reuse {
                    used.insert(file.path.clone());
                }
            }
            _ => report.unmatched.push(reference.clone()),
        }
    }

    report
}

/// Find missing project samples and reconnect them against a scan report.
pub fn reconnect_project(
    project: &mut Project,
    scan: &ScanReport,
    options: &ReconnectOptions,
) -> ReconnectionReport {
    let references = project
        .samples
        .iter()
        .map(|sample| FileReference::new(sample.path.clone(), PathBuf::from(&sample.path)))
        .collect::<Vec<_>>();
    let missing = find_missing_files(&references);
    let report = plan_reconnection(&missing, &scan.files, options);
    report.apply_to_project(project);
    report
}

fn file_name_lower(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn file_stem_lower(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn csv_document(header: &[String], rows: &[Vec<String>]) -> String {
    let mut out = header
        .iter()
        .map(|value| csv_escape(value))
        .collect::<Vec<_>>()
        .join(",");
    out.push('\n');
    for row in rows {
        out.push_str(
            &row.iter()
                .map(|value| csv_escape(value))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    out
}

fn sql_identifier(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "scan_results".to_string()
    } else {
        sanitized
    }
}

fn sql_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn sql_integer(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "NULL".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcs::FCS_HEADER_LEN;
    use crate::project::ProjectSample;
    use ndarray::Array2;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "flowers-quality-{}-{tag}-{nanos}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create temp dir");
        directory
    }

    fn build_fcs_with(
        events: usize,
        params: usize,
        extra: &[(&str, &str)],
        range: f32,
        value: impl Fn(usize, usize) -> f32,
    ) -> Vec<u8> {
        let mut text = Vec::new();
        text.push(b'/');
        text.extend_from_slice(format!("$TOT/{events}/").as_bytes());
        text.extend_from_slice(format!("$PAR/{params}/").as_bytes());
        for n in 1..=params {
            text.extend_from_slice(format!("$P{n}N/P{n}/").as_bytes());
            text.extend_from_slice(format!("$P{n}R/{range}/").as_bytes());
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
        for event in 0..events {
            for channel in 0..params {
                buffer.extend_from_slice(&value(event, channel).to_le_bytes());
            }
        }
        buffer
    }

    fn build_fcs(events: usize, params: usize, extra: &[(&str, &str)]) -> Vec<u8> {
        build_fcs_with(events, params, extra, 4.0, |event, _| {
            (event % 100) as f32 / 100.0
        })
    }

    #[test]
    fn quality_flags_empty_and_low_event_samples() {
        let empty = FcsFile {
            version: "FCS3.0".to_string(),
            keywords: HashMap::new(),
            parameters: Vec::new(),
            event_count: 0,
            data: Array2::zeros((0, 0)),
        };
        let thresholds = QualityThresholds::default();
        let report = SampleQualityControl::assess("empty", &empty, &thresholds);
        assert_eq!(report.status(), "fail");
        assert!(report.has_issue("no_events"));
        assert!(report.has_issue("no_parameters"));

        let bytes = build_fcs(
            50,
            1,
            &[("$CYT", "Test Cytometer"), ("$DATE", "01-JAN-2026")],
        );
        let parsed = FcsParser::parse_bytes(&bytes).expect("parse FCS");
        let report = SampleQualityControl::assess("small", &parsed, &thresholds);
        assert_eq!(report.status(), "warn");
        assert!(report.passed());
        assert!(report.has_issue("low_event_count"));
        assert!(!report.has_issue("missing_keyword"));
        assert_eq!(report.event_count, 50);
        assert_eq!(report.parameter_count, 1);
    }

    #[test]
    fn clean_sample_passes_quality_control() {
        let bytes = build_fcs(
            5_000,
            2,
            &[
                ("$CYT", "Test Cytometer"),
                ("$DATE", "01-JAN-2026"),
                ("$FIL", "clean.fcs"),
            ],
        );
        let path = temp_dir("clean").join("clean.fcs");
        fs::write(&path, &bytes).expect("write FCS");
        let thresholds =
            QualityThresholds::default().with_required_keywords(["$TOT", "$PAR", "$CYT", "$DATE"]);
        let report =
            SampleQualityControl::assess_file("clean", &path, &thresholds).expect("assess file");
        assert_eq!(report.status(), "pass");
        assert!(report.issues.is_empty());
        assert_eq!(report.path.as_deref(), Some(path.as_path()));
        assert_eq!(report.fcs_version, "FCS3.0");
        assert_eq!(report.channel_cvs.len(), 2);
    }

    #[test]
    fn scan_finds_nested_files_and_supports_search() {
        let root = temp_dir("scan");
        let nested = root.join("sub");
        fs::create_dir_all(&nested).expect("nested dir");
        fs::write(
            root.join("a.fcs"),
            build_fcs(100, 1, &[("$FIL", "a.fcs"), ("$CYT", "Cytometer A")]),
        )
        .expect("write a");
        fs::write(
            nested.join("b.fcs"),
            build_fcs(250, 2, &[("$FIL", "b.fcs"), ("$CYT", "Cytometer B")]),
        )
        .expect("write b");
        fs::write(root.join("notes.txt"), b"ignore me").expect("write notes");

        let options = ScanOptions {
            thresholds: QualityThresholds::default().with_min_events(0),
            ..ScanOptions::default()
        };
        let report = scan_directory(&root, &options).expect("scan");
        assert_eq!(report.len(), 2);
        assert_eq!(report.root, root);
        assert_eq!(report.parse_errors().count(), 0);
        let names = report
            .files
            .iter()
            .map(|file| file.file_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["a.fcs", "b.fcs"]);
        assert_eq!(report.files[1].event_count, Some(250));

        assert_eq!(
            report
                .search(&ScanQuery::default().with_text("b.fcs"))
                .len(),
            1
        );
        assert_eq!(
            report
                .search(
                    &ScanQuery::default()
                        .with_criterion(GroupCriterion::equals("$CYT", "Cytometer A"))
                )
                .len(),
            1
        );
        assert_eq!(
            report
                .search(&ScanQuery::default().with_event_range(200, 300))
                .len(),
            1
        );
        assert_eq!(
            report
                .search(&ScanQuery::default().with_version("FCS3"))
                .len(),
            2
        );
        assert_eq!(
            report
                .search(
                    &ScanQuery::default().with_criterion(GroupCriterion::equals("$CYT", "missing"))
                )
                .len(),
            0
        );

        let non_recursive = scan_directory(
            &root,
            &ScanOptions {
                recursive: false,
                thresholds: QualityThresholds::default().with_min_events(0),
                ..ScanOptions::default()
            },
        )
        .expect("non-recursive scan");
        assert_eq!(non_recursive.len(), 1);
    }

    #[test]
    fn scan_detects_identical_and_same_name_duplicates() {
        let root = temp_dir("duplicates");
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).expect("first dir");
        fs::create_dir_all(&second).expect("second dir");
        let payload = build_fcs(10, 1, &[("$CYT", "Cytometer")]);
        fs::write(first.join("copy.fcs"), &payload).expect("copy one");
        fs::write(second.join("copy.fcs"), &payload).expect("copy two");
        fs::write(
            first.join("sample.fcs"),
            build_fcs(10, 1, &[("$CYT", "Cytometer"), ("$SMNO", "1")]),
        )
        .expect("same name one");
        fs::write(
            second.join("sample.fcs"),
            build_fcs(10, 1, &[("$CYT", "Cytometer"), ("$SMNO", "2")]),
        )
        .expect("same name two");

        let report = scan_directory(
            &root,
            &ScanOptions {
                thresholds: QualityThresholds::default().with_min_events(0),
                ..ScanOptions::default()
            },
        )
        .expect("scan");
        assert_eq!(report.len(), 4);
        assert!(
            report
                .duplicates
                .iter()
                .any(|group| group.kind == DuplicateKind::IdenticalContent && group.len() == 2)
        );
        assert!(report.duplicates.iter().any(|group| {
            group.kind == DuplicateKind::SameNameAndSize
                && group.paths.iter().any(|path| path.ends_with("sample.fcs"))
                && group.len() == 2
        }));
        let sample_duplicates = report
            .duplicates
            .iter()
            .find(|group| group.kind == DuplicateKind::SameNameAndSize)
            .expect("same-name duplicate group");
        assert!(sample_duplicates.signature.starts_with("sample.fcs:"));
    }

    #[test]
    fn missing_files_are_reconnected_by_name_and_keywords() {
        let root = temp_dir("reconnect");
        let payload = build_fcs(
            500,
            1,
            &[
                ("$FIL", "sample.fcs"),
                ("$CYT", "Cytometer"),
                ("$DATE", "01-JAN-2026"),
                ("$BTIM", "12:00:00"),
            ],
        );
        fs::write(root.join("sample.fcs"), &payload).expect("write sample");
        let scan = scan_directory(
            &root,
            &ScanOptions {
                thresholds: QualityThresholds::default().with_min_events(0),
                ..ScanOptions::default()
            },
        )
        .expect("scan");
        let missing = FileReference::new("s1", root.join("old").join("sample.fcs"))
            .with_size(scan.files[0].size)
            .with_keyword("$TOT", "500")
            .with_keyword("$DATE", "01-JAN-2026");
        assert!(missing.is_missing());
        assert_eq!(find_missing_files(std::slice::from_ref(&missing)).len(), 1);

        let report = plan_reconnection(
            std::slice::from_ref(&missing),
            &scan.files,
            &ReconnectOptions::default(),
        );
        assert_eq!(report.len(), 1);
        assert!(report.unmatched.is_empty());
        let candidate = report.best_for("s1").expect("candidate");
        assert_eq!(candidate.candidate_path, root.join("sample.fcs"));
        assert!(candidate.score >= 100);
        assert!(candidate.reasons.contains(&"$TOT".to_string()));
        assert!(candidate.confidence() >= 1.0);

        let mut references = vec![missing];
        assert_eq!(report.apply(&mut references), 1);
        assert_eq!(references[0].path, root.join("sample.fcs"));
    }

    #[test]
    fn project_reconnection_updates_saved_paths() {
        let root = temp_dir("project-reconnect");
        fs::write(
            root.join("sample.fcs"),
            build_fcs(10, 1, &[("$CYT", "Cytometer")]),
        )
        .expect("write");
        let scan = scan_directory(
            &root,
            &ScanOptions {
                thresholds: QualityThresholds::default().with_min_events(0),
                ..ScanOptions::default()
            },
        )
        .expect("scan");
        let mut project = Project::default();
        project.samples.push(ProjectSample {
            path: root.join("gone").join("sample.fcs").display().to_string(),
            apply_gates: true,
            compensation: None,
            transforms: Vec::new(),
        });
        let report = reconnect_project(&mut project, &scan, &ReconnectOptions::default());
        assert_eq!(report.len(), 1);
        assert_eq!(
            project.samples[0].path,
            root.join("sample.fcs").display().to_string()
        );
    }

    #[test]
    fn scan_results_export_to_csv_xlsx_sql_and_workspace() {
        let root = temp_dir("export");
        fs::write(
            root.join("sample.fcs"),
            build_fcs(
                10,
                1,
                &[
                    ("$FIL", "sample.fcs"),
                    ("$CYT", "Cytometer"),
                    ("$DATE", "01-JAN-2026"),
                ],
            ),
        )
        .expect("write");
        let report = scan_directory(
            &root,
            &ScanOptions {
                thresholds: QualityThresholds::default().with_min_events(0),
                ..ScanOptions::default()
            },
        )
        .expect("scan");

        let csv = report.to_csv();
        assert!(
            csv.starts_with(
                "File,Path,Status,Version,Events,Parameters,Size,Hash,Duplicate,Issues"
            )
        );
        assert!(csv.contains("sample.fcs"));
        let csv_path = root.join("scan.csv");
        report.write_csv(&csv_path).expect("write csv");
        assert!(
            fs::read_to_string(&csv_path)
                .expect("read csv")
                .contains("sample.fcs")
        );

        let xlsx = report.to_xlsx().expect("xlsx");
        assert!(xlsx.starts_with(b"PK"));
        let xlsx_path = root.join("scan.xlsx");
        report.write_xlsx(&xlsx_path).expect("write xlsx");
        assert!(fs::metadata(&xlsx_path).expect("xlsx metadata").len() > 100);

        let sql = report.to_sql("fcs_scan");
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS fcs_scan"));
        assert!(sql.contains("INSERT INTO fcs_scan"));
        assert!(sql.contains("'sample.fcs'"));
        let sql_path = root.join("scan.sql");
        report.write_sql(&sql_path, "fcs_scan").expect("write sql");
        assert!(
            fs::read_to_string(&sql_path)
                .expect("read sql")
                .contains("fcs_scan")
        );

        let mut workspace = Workspace::new();
        let added = report.add_to_workspace(&mut workspace).expect("workspace");
        assert_eq!(added, ["sample.fcs"]);
        assert_eq!(workspace.samples.len(), 1);
        assert_eq!(workspace.samples["sample.fcs"].fcs_file.event_count, 10);

        let built = report.to_workspace().expect("to workspace");
        assert_eq!(built.sample_order, ["sample.fcs"]);

        let quality = report.quality_report();
        assert_eq!(quality.len(), 1);
        assert!(
            quality
                .to_csv()
                .starts_with("Sample,Path,Status,Version,Events")
        );
    }

    #[test]
    fn scan_query_reports_parse_errors_and_quality_failures() {
        let root = temp_dir("errors");
        fs::write(root.join("broken.fcs"), b"not an FCS file").expect("write broken");
        let report = scan_directory(
            &root,
            &ScanOptions {
                thresholds: QualityThresholds::default().with_min_events(0),
                ..ScanOptions::default()
            },
        )
        .expect("scan");
        assert_eq!(report.len(), 1);
        assert_eq!(report.files[0].status(), "error");
        assert!(report.files[0].parse_error.is_some());
        assert_eq!(
            report
                .search(&ScanQuery::default().failing_quality_only())
                .len(),
            0
        );
        let query = ScanQuery {
            include_parse_errors: false,
            ..ScanQuery::default()
        };
        assert_eq!(report.search(&query).len(), 0);
    }
}
