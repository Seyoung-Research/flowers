//! GUI project / workspace files (`.wsp`).
//!
//! A project stores the *analysis*, not the raw data: the list of FCS file
//! paths plus the gating strategy, view settings and layout. Loading a project
//! re-reads the referenced files, so only paths are persisted (FCS files are
//! large and usually live alongside the project).
//!
//! The module itself is independent of `egui`; the GUI converts between these
//! plain types and its in-memory state.

use serde::{Deserialize, Serialize};

use crate::compensation::CompensationMatrix;
use crate::derived::DerivedParameter;
use crate::statistics::StatisticSet;
use crate::transform::AxisScale;

/// Current on-disk project format version.
pub const PROJECT_VERSION: u32 = 1;

/// A saved analysis. All fields have defaults so that older / partial files can
/// still be read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    #[serde(default)]
    pub samples: Vec<ProjectSample>,
    #[serde(default)]
    pub current_sample: Option<usize>,
    #[serde(default)]
    pub x_param: Option<usize>,
    #[serde(default)]
    pub y_param: Option<usize>,
    #[serde(default)]
    pub selected_gate: Option<usize>,
    #[serde(default)]
    pub view: ViewSettings,
    #[serde(default)]
    pub gates: Vec<ProjectGate>,
    #[serde(default)]
    pub layout: Vec<ProjectPanel>,
    #[serde(default = "default_layout_columns")]
    pub layout_columns: usize,
    /// Named statistic sets configured for this analysis.
    #[serde(default)]
    pub statistic_sets: Vec<StatisticSet>,
    /// Formula-based derived parameters (T6).
    #[serde(default)]
    pub derived: Vec<DerivedParameter>,
}

fn default_layout_columns() -> usize {
    2
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: PROJECT_VERSION,
            samples: Vec::new(),
            current_sample: None,
            x_param: None,
            y_param: None,
            selected_gate: None,
            view: ViewSettings::default(),
            gates: Vec::new(),
            layout: Vec::new(),
            layout_columns: default_layout_columns(),
            statistic_sets: Vec::new(),
            derived: Vec::new(),
        }
    }
}

/// One referenced FCS file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSample {
    /// Path to the FCS file, as it was when the project was saved.
    pub path: String,
    /// Whether the shared gate set applies to this file.
    #[serde(default = "default_true")]
    pub apply_gates: bool,
    /// Compensation matrix applied to this sample, if any.
    #[serde(default)]
    pub compensation: Option<CompensationMatrix>,
    /// Per-parameter axis transforms configured for this sample; an empty list
    /// means the view-level (global) scale applies.
    #[serde(default)]
    pub transforms: Vec<(usize, AxisScale)>,
}

fn default_true() -> bool {
    true
}

/// Plot appearance / axis settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewSettings {
    /// One of `dots`, `contour`, `histogram`.
    pub mode: String,
    /// Axis transform key: `linear`, `log`, `arcsinh:150`, `biex:...`, etc.
    pub x_scale: String,
    /// Axis transform key: `linear`, `log`, `arcsinh:150`, `biex:...`, etc.
    pub y_scale: String,
    /// RGB point colour.
    pub point_color: [u8; 3],
    /// Density gradient layering.
    pub gradient: bool,
    /// Draw other populations of the current X/Y pair as coloured overlays.
    #[serde(default)]
    pub overlay_populations: bool,
    /// Density colour map for pseudocolour / zebra views (`rainbow`, `jet`,
    /// `viridis`, `greyscale`, `zebra`).
    #[serde(default = "default_density_map")]
    pub density_map: String,
    pub contour_levels: usize,
    pub x_range: (f64, f64),
    pub y_range: (f64, f64),
    /// Gate outline thickness in points.
    #[serde(default = "default_gate_line_width")]
    pub gate_line_width: f32,
    /// Draw a median-cross overlay for each visible gate population.
    #[serde(default)]
    pub show_median_cross: bool,
    /// Pull gate drawing toward dense regions (magnetic snapping).
    #[serde(default)]
    pub magnetic_snap: bool,
    /// Attraction strength in `[0, 1]`.
    #[serde(default = "default_magnetic_strength")]
    pub magnetic_strength: f32,
    /// Attraction radius as a fraction of the visible axis span.
    #[serde(default = "default_magnetic_radius_frac")]
    pub magnetic_radius_frac: f32,
    /// Population/gate overlay label options.
    #[serde(default)]
    pub gate_labels: crate::plot::GateLabelSettings,
    /// Automatic axis-range rules evaluated before data-driven auto-fit.
    #[serde(default)]
    pub autorange_rules: Vec<crate::transform::AutorangeRule>,
    /// Swap the X and Y axes in the main plot (V5).
    #[serde(default)]
    pub transpose: bool,
    /// Density smoothing fraction in `[0, 1]` (V5).
    #[serde(default)]
    pub plot_smoothing: f32,
    /// Density grid resolution in cells per axis (V5).
    #[serde(default = "default_plot_resolution")]
    pub plot_resolution: usize,
    /// Top percentage of events saturated on the density colour scale (V5).
    #[serde(default)]
    pub outlier_clip_percent: f64,
    /// Double the event marker radius (V5).
    #[serde(default)]
    pub large_dots: bool,
    /// Draw a background grid (V5).
    #[serde(default)]
    pub show_grid: bool,
    /// Draw median lines across the plot (V5).
    #[serde(default)]
    pub show_medians: bool,
    /// Show the plotted event count (V5).
    #[serde(default)]
    pub show_event_numbers: bool,
    /// Append frequencies to overlay names (V5).
    #[serde(default)]
    pub show_names_frequencies: bool,
    /// Tick notation key (`auto`, `scientific`, `decimal`).
    #[serde(default = "default_tick_notation")]
    pub tick_notation: String,
    /// Show X/Y axis labels (V5).
    #[serde(default = "default_true")]
    pub show_axis_labels: bool,
    /// Draw tick marks (V5).
    #[serde(default = "default_true")]
    pub show_ticks: bool,
    /// Draw tick numbers (V5).
    #[serde(default = "default_true")]
    pub show_numbers: bool,
    /// Draw adjunct X/Y histograms along the plot axes (V6).
    #[serde(default)]
    pub show_axis_histograms: bool,
}

fn default_gate_line_width() -> f32 {
    1.5
}

fn default_density_map() -> String {
    "rainbow".to_string()
}

fn default_plot_resolution() -> usize {
    128
}

fn default_tick_notation() -> String {
    "auto".to_string()
}

fn default_magnetic_strength() -> f32 {
    0.6
}

fn default_magnetic_radius_frac() -> f32 {
    0.04
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            mode: "dots".to_string(),
            x_scale: "linear".to_string(),
            y_scale: "linear".to_string(),
            point_color: [31, 119, 180],
            gradient: true,
            overlay_populations: false,
            density_map: default_density_map(),
            contour_levels: 8,
            x_range: (0.0, 1.0),
            y_range: (0.0, 1.0),
            gate_line_width: default_gate_line_width(),
            show_median_cross: false,
            magnetic_snap: false,
            magnetic_strength: default_magnetic_strength(),
            magnetic_radius_frac: default_magnetic_radius_frac(),
            gate_labels: crate::plot::GateLabelSettings::default(),
            autorange_rules: Vec::new(),
            transpose: false,
            plot_smoothing: 0.0,
            plot_resolution: default_plot_resolution(),
            outlier_clip_percent: 0.0,
            large_dots: false,
            show_grid: false,
            show_medians: false,
            show_event_numbers: false,
            show_names_frequencies: false,
            tick_notation: default_tick_notation(),
            show_axis_labels: true,
            show_ticks: true,
            show_numbers: true,
            show_axis_histograms: false,
        }
    }
}

/// A single gate, including its place in the hierarchy and its appearance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectGate {
    pub name: String,
    pub x_idx: usize,
    pub y_idx: usize,
    pub parent: Option<usize>,
    pub owner: Option<usize>,
    pub apply_to_all: bool,
    pub color: [u8; 3],
    pub kind: GateShape,
}

/// Serializable gate geometry, mirroring the GUI's gate kinds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GateShape {
    Rect {
        x_min: f64,
        x_max: f64,
        y_min: f64,
        y_max: f64,
        angle: f64,
    },
    Ellipse {
        x_min: f64,
        x_max: f64,
        y_min: f64,
        y_max: f64,
        angle: f64,
    },
    Polygon {
        vertices: Vec<[f64; 2]>,
    },
    Quadrant {
        x: f64,
        y: f64,
        /// `upper_left` / `upper_right` / `lower_left` / `lower_right`.
        quad: String,
    },
    Bilinear {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        below: bool,
    },
    /// Events selected via an external mask (index-based).
    Mask {
        indices: Vec<usize>,
    },
}

/// One layout-editor panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPanel {
    pub sample: usize,
    pub x: usize,
    pub y: usize,
    /// `dots` / `contour` / `histogram`.
    pub view: String,
    pub gate: Option<usize>,
    pub title: String,
    /// Font size (in SVG units) of the panel's title.
    #[serde(default = "default_panel_title_size")]
    pub title_size: f32,
    /// Canvas rectangle `[x, y, w, h]`.
    pub rect: [f32; 4],
    pub x_label: ProjectLabel,
    pub y_label: ProjectLabel,
}

fn default_panel_title_size() -> f32 {
    13.0
}

/// Styling for one layout axis label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectLabel {
    pub text: Option<String>,
    pub size: f32,
    pub rotation_deg: f32,
    /// `sans` / `serif` / `mono`.
    pub font: String,
    pub color: [u8; 3],
}

impl Default for ProjectLabel {
    fn default() -> Self {
        Self {
            text: None,
            size: 11.0,
            rotation_deg: 0.0,
            font: "sans".to_string(),
            color: [90, 90, 90],
        }
    }
}

/// Serialises a project to pretty JSON.
pub fn to_json(project: &Project) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(project)
}

/// Parses a project from JSON.
pub fn from_json(text: &str) -> Result<Project, serde_json::Error> {
    serde_json::from_str(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_all_gate_shapes() {
        let project = Project {
            samples: vec![ProjectSample {
                path: "/tmp/a.fcs".to_string(),
                apply_gates: false,
                compensation: None,
                transforms: Vec::new(),
            }],
            gates: vec![
                ProjectGate {
                    name: "R".to_string(),
                    x_idx: 0,
                    y_idx: 1,
                    parent: None,
                    owner: Some(0),
                    apply_to_all: true,
                    color: [255, 140, 0],
                    kind: GateShape::Rect {
                        x_min: 1.0,
                        x_max: 2.0,
                        y_min: 3.0,
                        y_max: 4.0,
                        angle: 0.5,
                    },
                },
                ProjectGate {
                    name: "L".to_string(),
                    x_idx: 0,
                    y_idx: 1,
                    parent: Some(0),
                    owner: None,
                    apply_to_all: false,
                    color: [0, 170, 220],
                    kind: GateShape::Bilinear {
                        x1: 1.0,
                        y1: 2.0,
                        x2: 5.0,
                        y2: 6.0,
                        below: true,
                    },
                },
                ProjectGate {
                    name: "Q".to_string(),
                    x_idx: 2,
                    y_idx: 3,
                    parent: None,
                    owner: None,
                    apply_to_all: true,
                    color: [40, 190, 80],
                    kind: GateShape::Quadrant {
                        x: 10.0,
                        y: 20.0,
                        quad: "upper_right".to_string(),
                    },
                },
            ],
            layout: vec![ProjectPanel {
                sample: 0,
                x: 0,
                y: 1,
                view: "dots".to_string(),
                gate: Some(0),
                title: "Panel".to_string(),
                title_size: 13.0,
                rect: [10.0, 20.0, 300.0, 200.0],
                x_label: ProjectLabel {
                    text: Some("FSC".to_string()),
                    ..Default::default()
                },
                y_label: ProjectLabel::default(),
            }],
            ..Default::default()
        };
        let json = to_json(&project).unwrap();
        let back = from_json(&json).unwrap();
        assert_eq!(back.gates.len(), 3);
        assert_eq!(back.samples[0].path, "/tmp/a.fcs");
        assert!(!back.samples[0].apply_gates);
        assert_eq!(back.layout.len(), 1);
        assert_eq!(back.layout[0].rect, [10.0, 20.0, 300.0, 200.0]);
        assert_eq!(back.gates[1].parent, Some(0));
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        // A minimal file only carrying the version.
        let back = from_json(r#"{"version":1}"#).unwrap();
        assert!(back.samples.is_empty());
        assert_eq!(back.view.mode, "dots");
        assert_eq!(back.view.x_scale, "linear");
        assert_eq!(back.view.y_scale, "linear");
        assert_eq!(back.view.point_color, [31, 119, 180]);
        assert_eq!(back.layout_columns, 2);
        assert!(back.statistic_sets.is_empty());
        assert_eq!(back.view.gate_line_width, 1.5);
        assert!(!back.view.show_median_cross);
        assert!(!back.view.magnetic_snap);
        assert_eq!(back.view.magnetic_strength, 0.6);
        assert_eq!(back.view.magnetic_radius_frac, 0.04);
        assert!(back.view.gate_labels.show);
        assert!(back.view.gate_labels.name);
        assert!(!back.view.gate_labels.ancestry);
        assert!(!back.view.gate_labels.backgating);
    }

    #[test]
    fn gate_style_view_settings_round_trip() {
        let mut project = Project::default();
        project.view.gate_line_width = 3.5;
        project.view.show_median_cross = true;
        project.view.magnetic_snap = true;
        project.view.magnetic_strength = 0.85;
        project.view.magnetic_radius_frac = 0.08;
        project.view.gate_labels.ancestry = true;
        project.view.gate_labels.backgating = true;
        let json = to_json(&project).unwrap();
        let back = from_json(&json).unwrap();
        assert_eq!(back.view.gate_line_width, 3.5);
        assert!(back.view.show_median_cross);
        assert!(back.view.magnetic_snap);
        assert_eq!(back.view.magnetic_strength, 0.85);
        assert_eq!(back.view.magnetic_radius_frac, 0.08);
        assert!(back.view.gate_labels.ancestry);
        assert!(back.view.gate_labels.backgating);
    }

    #[test]
    fn statistic_sets_round_trip() {
        let project = Project {
            statistic_sets: vec![
                crate::statistics::StatisticSet::robust(),
                crate::statistics::StatisticSet::new(
                    "Custom",
                    vec![crate::statistics::StatSpec {
                        parameter: Some("CD4".to_string()),
                        statistic: crate::statistics::Statistic::Median,
                        expected: None,
                        heat: None,
                    }],
                ),
            ],
            ..Default::default()
        };
        let json = to_json(&project).unwrap();
        let back = from_json(&json).unwrap();
        assert_eq!(back.statistic_sets, project.statistic_sets);
        assert_eq!(back.statistic_sets[0].name, "Robust");
    }

    #[test]
    fn view_settings_round_trip_all_axis_scale_keys() {
        for key in [
            "linear",
            "log",
            "arcsinh:150",
            "biex:100,4.5,1,0.5,0",
            "logicle:100,4.5,1,0.5,0",
            "hyperlog:100,4.5,0",
            "robustlog:10",
            "miltenyi:10",
        ] {
            let json = format!(
                r#"{{
                    "version": 1,
                    "view": {{
                        "mode": "dots",
                        "x_scale": "{key}",
                        "y_scale": "{key}",
                        "point_color": [31, 119, 180],
                        "gradient": true,
                        "contour_levels": 8,
                        "x_range": [0, 1],
                        "y_range": [0, 1]
                    }}
                }}"#
            );
            let back = from_json(&json).unwrap();
            let parsed = crate::transform::AxisScale::from_key(key);
            assert_eq!(
                back.view.x_scale, key,
                "axis scale {key} did not round-trip"
            );
            assert_eq!(back.view.y_scale, key);
            assert_eq!(parsed.key(), key);
        }
    }
}
