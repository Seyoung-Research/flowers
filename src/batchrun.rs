//! Command-line batch / protocol execution (Phase 9, X3).

use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

use crate::compensation::{CompensationError, CompensationMatrix};
use crate::quality::{
    QualityControlReport, QualityError, QualityThresholds, SampleQualityControl, ScanOptions,
    scan_directory,
};
use crate::workspace::Workspace;

/// Errors produced by batch runs.
#[derive(Debug, Error)]
pub enum BatchError {
    #[error("IO error: {0}")]
    Io(std::io::Error),
    #[error("quality error: {0}")]
    Quality(#[from] QualityError),
    #[error("compensation error: {0}")]
    Compensation(#[from] CompensationError),
}

/// Options of a batch / protocol execution.
#[derive(Debug, Clone, Default)]
pub struct BatchOptions {
    /// Optional workspace file to start from (later steps add to it).
    pub workspace: Option<PathBuf>,
    /// Optional data folder scanned for FCS files.
    pub data_folder: Option<PathBuf>,
    /// Output folder for generated artifacts.
    pub output_folder: Option<PathBuf>,
    /// Generate one graph SVG per sample.
    pub generate_graphs: bool,
    /// Generate one compensation matrix per sample.
    pub generate_compensation: bool,
    /// Generate sample quality-control reports.
    pub generate_quality: bool,
    /// Export quality CSV reports.
    pub generate_csv: bool,
    /// Export an ACS workspace archive.
    pub generate_acs: bool,
}

impl BatchOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_data_folder(mut self, path: impl Into<PathBuf>) -> Self {
        self.data_folder = Some(path.into());
        self
    }

    pub fn with_output_folder(mut self, path: impl Into<PathBuf>) -> Self {
        self.output_folder = Some(path.into());
        self
    }

    pub fn with_generate_quality(mut self, generate_quality: bool) -> Self {
        self.generate_quality = generate_quality;
        self
    }
}

/// Aggregated outcome of a batch run.
#[derive(Debug, Clone, Default)]
pub struct BatchResult {
    pub scanned: usize,
    pub graphs: usize,
    pub compensations: usize,
    pub quality_reports: usize,
    pub csvs: usize,
    pub archives: usize,
    pub errors: Vec<String>,
}

/// Context handed to a batch run: the workspace plus scan/output locations.
#[derive(Debug, Clone)]
pub struct BatchContext {
    pub workspace: Workspace,
    pub data_folder: Option<PathBuf>,
    pub output_folder: Option<PathBuf>,
}

/// Serialised compensation matrix artifact.
#[derive(Debug, Serialize)]
struct CompensationExport {
    channels: Vec<String>,
    values: Vec<Vec<f32>>,
}

/// Command-line batch / protocol runner.
pub struct BatchRunner {
    options: BatchOptions,
}

impl BatchRunner {
    pub fn new(options: BatchOptions) -> Self {
        Self { options }
    }

    /// Run the configured batch steps over the context.
    pub fn run(&self, context: &mut BatchContext) -> Result<BatchResult, BatchError> {
        let mut result = BatchResult::default();

        // 1. Scan the data folder (when configured).
        if let Some(folder) = &context.data_folder {
            let report = scan_directory(folder, &ScanOptions::default())?;
            result.scanned = report.len();
            if self.options.generate_quality {
                let report = QualityControlReport {
                    reports: report
                        .files
                        .iter()
                        .filter_map(|file| file.quality.clone())
                        .collect(),
                };
                if let Some(output) = &context.output_folder {
                    let path = output.join("quality_report.csv");
                    report.write_csv(&path)?;
                }
                result.quality_reports = report.reports.len();
            }
        }

        // 2. Per-sample artifacts over the workspace.
        for id in context.workspace.sample_order.clone() {
            let Some(sample) = context.workspace.samples.get(&id) else {
                continue;
            };
            if self.options.generate_graphs {
                let svg = sample_graph_svg(&sample.fcs_file);
                if let Some(output) = &context.output_folder {
                    fs::write(output.join(format!("{id}.svg")), svg).map_err(BatchError::Io)?;
                }
                result.graphs += 1;
            }
            if self.options.generate_compensation {
                let channels = sample
                    .fcs_file
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect::<Vec<String>>();
                if let Ok(matrix) = CompensationMatrix::identity(channels) {
                    let export = CompensationExport {
                        channels: matrix.channels.clone(),
                        values: matrix.values.clone(),
                    };
                    let text = serde_json::to_string_pretty(&export).map_err(|error| {
                        BatchError::Compensation(CompensationError::InvalidMatrix(
                            error.to_string(),
                        ))
                    })?;
                    if let Some(output) = &context.output_folder {
                        fs::write(output.join(format!("{id}_compensation.json")), text)
                            .map_err(BatchError::Io)?;
                    }
                    result.compensations += 1;
                }
            }
            if self.options.generate_quality {
                let mut report = QualityControlReport::new();
                report.add(SampleQualityControl::assess(
                    id.clone(),
                    &sample.fcs_file,
                    &QualityThresholds::default(),
                ));
                if let Some(output) = &context.output_folder {
                    report.write_csv(output.join("quality_report.csv"))?;
                }
                result.quality_reports += 1;
                result.csvs += 1;
            }
        }

        // 3. ACS archive export.
        if self.options.generate_acs {
            let archive =
                crate::archive::WorkspaceArchive::new("batch", crate::project::Project::default());
            let text = archive.to_json().map_err(|error| {
                BatchError::Compensation(CompensationError::InvalidMatrix(error.to_string()))
            })?;
            if let Some(output) = &context.output_folder {
                fs::write(output.join("batch.acs"), text).map_err(BatchError::Io)?;
            }
            result.archives += 1;
        }

        Ok(result)
    }
}

/// Render a minimal two-channel scatter plot SVG for one sample.
fn sample_graph_svg(fcs: &crate::fcs::FcsFile) -> String {
    let events = fcs.data.nrows();
    let channels = fcs.data.ncols().clamp(1, 2);
    let mut points = String::new();
    if events > 0 && channels >= 2 {
        for event in 0..events {
            let row = fcs.data.row(event);
            let x = row[0];
            let y = row[1];
            points.push_str(&format!("{x},{y} "));
        }
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"640\" height=\"480\" viewBox=\"0 0 640 480\"><rect width=\"640\" height=\"480\" fill=\"#ffffff\"/><polyline points=\"0,0 {points}\" fill=\"none\" stroke=\"black\" stroke-width=\"1\"/></svg>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_runner_reports_zero_steps_for_empty_workspace() {
        let mut context = BatchContext {
            workspace: Workspace::new(),
            data_folder: None,
            output_folder: None,
        };
        let runner = BatchRunner::new(BatchOptions::new());
        let result = runner.run(&mut context).expect("run batch");
        assert_eq!(result.scanned, 0);
        assert_eq!(result.graphs, 0);
        assert_eq!(result.compensations, 0);
        assert_eq!(result.quality_reports, 0);
        assert_eq!(result.csvs, 0);
        assert_eq!(result.archives, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn batch_runner_writes_quality_csv_and_acs_for_sample() {
        let mut workspace = Workspace::new();
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
        let output = std::env::temp_dir().join(format!("flowers-batch-{}-out", std::process::id()));
        let _ = fs::create_dir_all(&output);
        let mut context = BatchContext {
            workspace,
            data_folder: None,
            output_folder: Some(output),
        };
        let mut options = BatchOptions::new();
        options.generate_quality = true;
        options.generate_acs = true;
        let runner = BatchRunner { options };
        let result = runner.run(&mut context).expect("run batch");
        assert_eq!(result.quality_reports, 1);
        assert_eq!(result.csvs, 1);
        assert_eq!(result.archives, 1);
        assert_eq!(result.graphs, 0);
        assert_eq!(result.compensations, 0);
    }

    #[test]
    fn batch_runner_generates_graph_and_compensation_per_sample() {
        let mut workspace = Workspace::new();
        workspace.add_sample(
            "s1",
            crate::fcs::FcsFile {
                version: "FCS3.0".to_string(),
                keywords: Default::default(),
                parameters: vec![crate::fcs::Parameter {
                    name: "FL1".to_string(),
                    description: "FITC".to_string(),
                    range: 1024.0,
                    exponent: 0.0,
                    min: 0.0,
                    max: 1024.0,
                }],
                event_count: 0,
                data: ndarray::Array2::<f32>::zeros((0, 0)),
            },
        );
        let output =
            std::env::temp_dir().join(format!("flowers-batch-{}-out2", std::process::id()));
        let _ = fs::create_dir_all(&output);
        let mut context = BatchContext {
            workspace,
            data_folder: None,
            output_folder: Some(output),
        };
        let mut options = BatchOptions::new();
        options.generate_graphs = true;
        options.generate_compensation = true;
        let runner = BatchRunner { options };
        let result = runner.run(&mut context).expect("run batch");
        assert_eq!(result.graphs, 1);
        assert_eq!(result.compensations, 1);
        let _ = fs::remove_dir_all(
            context
                .output_folder
                .as_deref()
                .unwrap_or(std::path::Path::new("")),
        );
    }
}
