//! `flowers` — an open-source flow cytometry analysis tool.
//!
//! Core capabilities:
//! - FCS 2.0 / 3.0 file parsing (`fcs`)
//! - Compensation / spillover correction (`compensation`)
//! - Gating: rectangular, ellipse, polygon, quadrant, bilinear, boolean,
//!   range, time, mask, file-mask, tethered, spider and curly-quadrant gates, plus
//!   gate conversion, scaling and median-cross overlays (`gating`), plus
//!   density-based magnetic snapping while drawing (`magnetic`)
//! - Population statistics: mean, median, SD, CV, geometric mean, min, max,
//!   mode, percentiles, robust SD/CV and CDF (`statistics`)
//! - Axis transforms: linear, log, arcsinh, biexponential/logicle,
//!   hyperlog, robust log and Miltenyi (`transform`)
//! - Experiment workspace & project files (`workspace`)
//! - SVG visualization: dot plots, histograms, density, contours (`plot`)
//!
//! See `src/main.rs` for the command-line interface.

pub mod archive;
pub mod autogating;
pub mod autosave;
pub mod batch;
pub mod batchrun;
pub mod compensation;
pub mod derived;
pub mod diva;
pub mod editor;
pub mod export;
pub mod fcs;
pub mod flowjo;
pub mod gating;
pub mod gatingml;
pub mod groups;
pub mod gui;
pub mod layout;
pub mod magnetic;
pub mod merge;
pub mod plate;
pub mod plot;
pub mod plot3d;
pub mod plugin;
pub mod preferences;
pub mod project;
pub mod quality;
pub mod raster;
pub mod report;
pub mod repositories;
pub mod scripting;
pub mod statistics;
pub mod table;
pub mod transform;
pub mod workspace;

pub use archive::{
    ArchiveError, TemplateSample, WorkspaceArchive, WorkspaceTemplate, WorkspaceTemplateRegistry,
};
pub use autogating::{
    Bifurcation, Cluster, Peak, bifurcate, cluster_gates, convex_hull, find_peaks, gate_on_peak,
    kmeans_clusters, polygon_autogate,
};
pub use autosave::{
    AutosaveError, AutosaveSettings, BatchedSave, SavedState, Scrapbook, ScrapbookEntry,
    UndoHistory, WorkspaceSnapshot, load_snapshot_file, save_snapshot_file,
};
pub use batchrun::{BatchContext, BatchError, BatchOptions, BatchResult, BatchRunner};
pub use compensation::{
    CompensationComparison, CompensationEngine, CompensationError, CompensationMatrix,
    CompensationValidation, MatrixType, cosine_similarity, solve_ols, spreading_matrix,
    total_spread,
};
pub use derived::{DerivedError, DerivedParameter, Expr};
pub use diva::{DivaError, DivaExperiment, DivaSample};
pub use editor::{KeywordReplaceReport, SampleEditor};
pub use export::{PopulationRow, populations_csv, write_populations_csv};
pub use fcs::{FcsError, FcsFile, FcsParser, Parameter};
pub use flowjo::{
    FlowJoError, FlowJoPopulation, FlowJoSample, FlowJoWorkspaceDocument,
    export_flowjo_workspace_json, export_jot, import_flowjo_workspace_json, import_jot,
};
pub use gating::{
    BilinearGate, BooleanGate, CurlyQuadrantGate, EllipseGate, FileMaskGate, Gate, GateCombination,
    GateGeometry, GateRefError, GateSet, GateShape, GateStyle, LineGate, MaskError, MaskFileFormat,
    MaskGate, PolygonGate, Quadrant, QuadrantGate, RangeGate, RectangularGate, SpiderGate,
    TetherBound, TetherStatistic, TetheredRangeGate, TimeGate, TimeGateMode, median_cross,
};
pub use gatingml::{
    BooleanExpr, GatingMlDocument, GatingMlError, GatingMlGate, GatingMlGateKind, GatingMlVersion,
    gatingml_to_shape, shape_to_gatingml,
};
pub use groups::{
    GroupCriterion, GroupGate, GroupLineStyle, GroupMarker, GroupRole, GroupStyle, SampleGroup,
    SampleGroups,
};
pub use layout::{LayoutOptions, LayoutPanel, LayoutPlacement, render_layout, render_layout_free};
pub use magnetic::{DensityField, SnapSettings, snap_polygon, snap_rectangle, snap_to_density};
pub use merge::{
    FcsBuilder, MergeError, MergeOptions, fcs_to_bytes, merge_fcs, merge_workspace_samples,
    population_rows, population_rows_for_sample, write_fcs,
};
pub use plate::{
    Plate, PlateFormat, PlateTemplate, PlateTemplateRegistry, Titration, TitrationLevel, WellId,
    WellMetadata,
};
pub use plot::{AxisScale, PlotOverlay, PlotStyle, density_grid, point_densities};
pub use plugin::{Plugin, PluginCapability, PluginHost};
pub use preferences::{
    AutorangePreferences, CommandLinePreferences, GatePreferences, LayoutPreferences, Preferences,
    TablePreferences, TransformPreferences,
};
pub use quality::{
    DuplicateGroup, DuplicateKind, FcsScanner, FileReference, QualityControlReport, QualityError,
    QualityIssue, QualitySeverity, QualitySummary, QualityThresholds, ReconnectOptions,
    ReconnectionCandidate, ReconnectionReport, SampleQualityControl, ScanOptions, ScanQuery,
    ScanReport, ScannedFile, assess_workspace, detect_duplicates, find_missing_files, hash_bytes,
    plan_reconnection, reconnect_project, scan_directory, scan_fcs_file, scan_fcs_files,
    score_reference,
};
pub use raster::{ImageFormat, rasterize_svg, save_svg_as_image};
pub use repositories::{
    ConnectionProfile, ListQuery, RepositoryConnector, RepositoryError, RepositoryHost,
    RepositoryKind, RepositoryMetadata, RepositorySample, RepositoryUploadReport,
};
pub use scripting::{Script, ScriptContext, ScriptError, ScriptingEngine, run_script};
pub use statistics::{PopulationProfile, PopulationStat, StatSpec, Statistic, StatisticSet};
pub use transform::{
    AutorangeLimit, AutorangeMatcher, AutorangeRule, BiexponentialParams, CustomTransform,
    LookupError, LookupPlugin, LookupTable, TransformPluginHost, TransformRegistry, TransformScope,
    percentile_of,
};
pub use workspace::{Population, Workspace};
