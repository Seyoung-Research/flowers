//! egui-based GUI for `flowers`.
//!
//! Provides a desktop application for loading FCS files, drawing gates on
//! scatter plots (rect / ellipse / quadrant / bilinear), organising gates as a
//! Hierarchical population tree, applying those gates to every loaded
//! sample, exporting CSV reports, and arranging plots in a "layout editor".
//!
//! Feature map (see `FEATURES.md`):
//! * fixed-size plotting window with editable X/Y ranges
//! * per-axis "Auto" scaling button
//! * per-axis linear / logarithmic scale
//! * configurable dot colour
//! * density gradient layering (points recoloured where they overlap)
//! * detachable window that shows only a gated population
//! * dots vs. topographic (contour) view
//! * gates applied across all loaded FCS files
//! * per-file "apply existing gates?" prompt when a new file is loaded
//! * default X/Y axes picked as FSC-H / SSC-H when present
//! * hierarchical gate tree with counts and percentages
//! * CSV export of counts / percentages
//! * quadrant gate (draw or click)
//! * bilinear / line gate (the region under a drawn line)
//! * dark / light theme switch
//! * detached population window with the full plotting & gating toolkit
//! * interactive gate manipulation (move / rotate / enlarge / shrink)
//! * free-form visual layout editor (drag, resize, reorder, preview)
//! * editable axis labels in the layout editor (text, size, rotation, font, colour)
//! * figure export to SVG, PNG, JPEG and TIFF

use eframe::egui;
use egui::load::{Bytes, BytesPoll, ImageLoader, ImagePoll, LoadError, SizeHint};
use egui::widgets::Widget;
use egui_plot::{Bar, BarChart, Line, Plot, PlotBounds, Points, Polygon};
use std::sync::Arc;

use crate::compensation::CompensationMatrix;
use crate::export::{PopulationRow, statistics_rows, write_populations_csv, write_statistics_csv};
use crate::fcs::{FcsFile, FcsParser};
use crate::gating::{
    EllipseGate, FileMaskGate, Gate as GatingGate, LineGate, MaskFileFormat, MaskGate, Quadrant,
    QuadrantGate, RectangularGate,
};
use crate::layout::{LayoutOptions, LayoutPlacement, render_layout_free};
use crate::plot::{
    AxisScale, GateLabelSettings, OverlayLabel, PlotOverlay, PlotStyle, PopulationLabel,
    backgate_ancestors, density_grid, density_rgb, dot_plot_svg, point_densities,
};
use crate::project::{
    GateShape, Project, ProjectGate, ProjectLabel, ProjectPanel, ProjectSample, ViewSettings,
};
use crate::raster::{ImageFormat, save_svg_as_image};
use crate::transform::{AutorangeRule, LookupTable};

// ---------------------------------------------------------------------------
// Flow_r-inspired design system
//
// Teal primary (#0f766e), slate neutrals, light page background, card
// containers with rounded corners, small uppercase labels. Colours adapt to
// the selected theme.
// ---------------------------------------------------------------------------

/// Colours for the current theme, mirroring the flow_r design tokens.
#[derive(Clone, Copy)]
struct Palette {
    bg: egui::Color32,         // page background
    surface: egui::Color32,    // card background
    border: egui::Color32,     // card / hairline border
    ink: egui::Color32,        // primary text
    muted: egui::Color32,      // secondary text
    teal: egui::Color32,       // primary accent
    teal_light: egui::Color32, // teal tint fill
    soft: egui::Color32,       // subtle fill (hover / active strips)
}

impl Palette {
    fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Dark => Palette {
                bg: egui::Color32::from_rgb(15, 23, 42),       // #0f172a
                surface: egui::Color32::from_rgb(30, 41, 59),  // #1e293b
                border: egui::Color32::from_rgb(51, 65, 85),   // #334155
                ink: egui::Color32::from_rgb(241, 245, 249),   // #f1f5f9
                muted: egui::Color32::from_rgb(148, 163, 184), // #94a3b8
                teal: egui::Color32::from_rgb(94, 234, 212),   // #5eead4
                teal_light: egui::Color32::from_rgb(15, 118, 110),
                soft: egui::Color32::from_rgb(38, 50, 69), // #263245
            },
            Theme::Light => Palette {
                bg: egui::Color32::from_rgb(244, 246, 249), // #f4f6f9
                surface: egui::Color32::WHITE,
                border: egui::Color32::from_rgb(226, 232, 240), // #e2e8f0
                ink: egui::Color32::from_rgb(15, 23, 42),       // #0f172a
                muted: egui::Color32::from_rgb(100, 116, 139),  // #64748b
                teal: egui::Color32::from_rgb(15, 118, 110),    // #0f766e
                teal_light: egui::Color32::from_rgb(204, 251, 241),
                soft: egui::Color32::from_rgb(248, 250, 252), // #f8fafc
            },
        }
    }
}

/// A card container: rounded, bordered, filled surface.
fn card_frame(p: Palette) -> egui::Frame {
    egui::Frame::none()
        .fill(p.surface)
        .stroke(egui::Stroke::new(1.0_f32, p.border))
        .rounding(10.0)
        .inner_margin(egui::Margin::same(10.0))
}

/// Padding between the window edge and every top-level panel's card.
const PANEL_MARGIN: f32 = 10.0;

/// The shared frame for every top-level panel (header, sidebars, bottom
/// editor and the central plot card), so their cards all sit the same
/// distance from the window edges.
fn panel_frame(p: Palette) -> egui::Frame {
    egui::Frame::none()
        .fill(p.bg)
        .inner_margin(egui::Margin::same(PANEL_MARGIN))
}

/// A card header: bold title on the left, optional muted subtitle / trailing
/// content on the right, followed by a hairline rule.
fn card_head(ui: &mut egui::Ui, p: Palette, title: &str, trailing: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong().small().color(p.ink));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            trailing(ui);
        });
    });
    ui.add_space(2.0);
    ui.separator();
}

/// A muted section label (like flow_r's "card-sub" / mini titles).
/// Uses the default text size so it stays consistent with dropdowns and
/// other controls rather than appearing smaller than them.
fn section_label(ui: &mut egui::Ui, p: Palette, text: &str) {
    ui.label(egui::RichText::new(text).color(p.muted));
}

/// A small, teal-accented primary button.
fn primary_button(ui: &mut egui::Ui, p: Palette, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).small().strong().color(p.surface))
            .fill(p.teal)
            .rounding(6.0),
    )
}

/// An outlined secondary button.
fn secondary_button(ui: &mut egui::Ui, p: Palette, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).small().color(p.ink))
            .fill(p.soft)
            .stroke(egui::Stroke::new(1.0_f32, p.border))
            .rounding(6.0),
    )
}

/// An outlined secondary button with hover text.
fn secondary_button_hover(
    ui: &mut egui::Ui,
    p: Palette,
    text: &str,
    hover: &str,
) -> egui::Response {
    let button = egui::Button::new(egui::RichText::new(text).small().color(p.ink))
        .fill(p.soft)
        .stroke(egui::Stroke::new(1.0_f32, p.border))
        .rounding(6.0);
    ui.add(button).on_hover_text(hover)
}

/// A toggle-style plot tool button.
fn tool_button(ui: &mut egui::Ui, p: Palette, label: &str, active: bool) -> egui::Response {
    let text = egui::RichText::new(label)
        .small()
        .strong()
        .color(if active { p.teal } else { p.muted });
    let btn = egui::Button::new(text).rounding(6.0);
    if active {
        ui.add_enabled(
            false,
            btn.fill(p.teal_light)
                .stroke(egui::Stroke::new(1.0_f32, p.teal)),
        )
    } else {
        ui.add(
            btn.fill(p.soft)
                .stroke(egui::Stroke::new(1.0_f32, p.border)),
        )
    }
}

/// An [`egui::ImageLoader`] that decodes images handed to egui over `bytes://` URIs.
///
/// The default egui context only wires up a default *texture* loader; that loader
/// delegates to image loaders, of which none are registered by default. Without a
/// registered image loader, every `bytes://` image load fails with
/// [`LoadError::NoImageLoaders`], and egui falls back to drawing its error
/// placeholder (the red "⚠" warning glyph) instead of the real image.
pub struct FlowersImageLoader;

impl FlowersImageLoader {
    const URI_PREFIX: &'static str = "bytes://";
}

impl ImageLoader for FlowersImageLoader {
    fn id(&self) -> &str {
        "flowers::FlowersImageLoader"
    }

    fn load(
        &self,
        ctx: &egui::Context,
        uri: &str,
        _size_hint: SizeHint,
    ) -> egui::load::ImageLoadResult {
        if !uri.starts_with(Self::URI_PREFIX) {
            return Err(LoadError::NotSupported);
        }

        let bytes_poll = ctx
            .try_load_bytes(uri)
            .map_err(|err| LoadError::Loading(err.to_string()))?;
        let bytes = match bytes_poll {
            BytesPoll::Ready { bytes, .. } => bytes,
            BytesPoll::Pending { .. } => {
                return Err(LoadError::Loading("logo bytes still loading".to_string()));
            }
        };
        let bytes: Vec<u8> = match bytes {
            Bytes::Static(static_bytes) => static_bytes.to_vec(),
            Bytes::Shared(shared) => shared.as_ref().to_vec(),
        };

        let image = image::load_from_memory(&bytes)
            .map_err(|err| LoadError::Loading(err.to_string()))?
            .to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &image);
        Ok(ImagePoll::Ready {
            image: Arc::new(color_image),
        })
    }

    fn forget(&self, _uri: &str) {}

    fn forget_all(&self) {}

    fn end_pass(&self, _frame_index: usize) {}

    fn byte_size(&self) -> usize {
        0
    }
}

/// Cached PNG encoding of the project `logo.jpeg`, used by the header brand mark.
fn logo_png_bytes() -> &'static [u8] {
    static CACHE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/logo.jpeg"));
            let img = image::open(path).expect("failed to open logo.jpeg");
            let mut buf = Vec::new();
            let mut cursor = std::io::Cursor::new(&mut buf);
            img.write_to(&mut cursor, image::ImageFormat::Png)
                .expect("failed to encode logo as png");
            buf
        })
        .as_slice()
}

/// egui image source for the project logo (`logo.jpeg`), cached across frames.
fn logo_image_source() -> egui::ImageSource<'static> {
    static CACHE: std::sync::OnceLock<egui::ImageSource<'static>> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            egui::ImageSource::from(("bytes://flowers-logo.png", logo_png_bytes().to_vec()))
        })
        .clone()
}

/// The header bar: brand mark + app name on the left, menu bar on the right.
fn header_bar(ui: &mut egui::Ui, p: Palette, menu: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        // Brand mark: the project logo image.
        egui::Frame::none()
            .fill(p.teal)
            .rounding(9.0)
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                egui::Image::new(logo_image_source())
                    .max_width(32.0)
                    .max_height(32.0)
                    .ui(ui);
            });
        ui.add_space(10.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("flowers").strong().color(p.ink));
            ui.label(
                egui::RichText::new("flow cytometry analysis")
                    .small()
                    .color(p.muted),
            );
        });
        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);
        egui::menu::bar(ui, menu);
    });
}

// ---------------------------------------------------------------------------
// Gate model
// ---------------------------------------------------------------------------

/// View mode for a plot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ViewMode {
    #[default]
    Dots,
    Contour,
    Histogram,
    Pseudocolor,
    Zebra,
}

impl ViewMode {
    fn label(self) -> &'static str {
        match self {
            ViewMode::Dots => "Dots",
            ViewMode::Contour => "Contour",
            ViewMode::Histogram => "Histogram",
            ViewMode::Pseudocolor => "Pseudocolor",
            ViewMode::Zebra => "Zebra",
        }
    }
}

/// Identifies a draggable manipulation handle on a gate.
#[derive(Debug, Clone, PartialEq)]
enum HandleId {
    /// Corner handle (0..4) for uniform scale about the centre.
    Corner(usize),
    /// Edge handle (0..3) for single-axis stretch of an ellipse.
    Edge(usize),
    /// Rotation handle.
    Rotate,
    /// Polygon vertex handle.
    Vertex(usize),
    /// Centre handle (move a split point / bilinear centre).
    Center,
}

/// A manipulation target chosen when a drag begins on a selected gate.
#[derive(Debug, Clone, PartialEq)]
enum ManipTarget {
    Move,
    Handle(HandleId),
}

/// The shape/geometry of a gate.
#[derive(Debug, Clone, PartialEq)]
enum GateKind {
    Rect {
        x_min: f64,
        x_max: f64,
        y_min: f64,
        y_max: f64,
        /// Rotation about the bounding-box centre, in radians.
        angle: f64,
    },
    Ellipse {
        x_min: f64,
        x_max: f64,
        y_min: f64,
        y_max: f64,
        /// Rotation about the bounding-box centre, in radians.
        angle: f64,
    },
    /// Free-form polygon (vertices in data coordinates).
    Polygon { vertices: Vec<[f64; 2]> },
    /// One wedge of a 4-way quadrant split.
    Quadrant { x: f64, y: f64, quad: Quadrant },
    /// A line gate: events on one side of a drawn line segment, restricted to
    /// the segment's horizontal extent (the "region under the line").
    Bilinear {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        /// When true, points below the line are selected; false selects above.
        below: bool,
    },
    /// Events selected through an external mask (index-based membership).
    ///
    /// Not drawable; used for imported mask gates. Geometry-free by design.
    Mask {
        /// Sorted, de-duplicated zero-based event indices.
        indices: Vec<usize>,
    },
}

impl GateKind {
    fn label(&self) -> &'static str {
        match self {
            GateKind::Rect { .. } => "Rect",
            GateKind::Ellipse { .. } => "Ellipse",
            GateKind::Polygon { .. } => "Polygon",
            GateKind::Quadrant { .. } => "Quadrant",
            GateKind::Bilinear { .. } => "Bilinear",
            GateKind::Mask { .. } => "Mask",
        }
    }

    /// Build the trait object used for event filtering.
    fn to_gate(&self, x_idx: usize, y_idx: usize) -> Box<dyn GatingGate> {
        match self {
            GateKind::Rect {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
            } => Box::new(
                RectangularGate::new(
                    x_idx,
                    y_idx,
                    *x_min as f32,
                    *x_max as f32,
                    *y_min as f32,
                    *y_max as f32,
                )
                .with_angle(*angle as f32),
            ),
            GateKind::Ellipse {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
            } => Box::new(
                EllipseGate::new(
                    x_idx,
                    y_idx,
                    ((x_min + x_max) / 2.0) as f32,
                    ((y_min + y_max) / 2.0) as f32,
                    ((x_max - x_min) / 2.0) as f32,
                    ((y_max - y_min) / 2.0) as f32,
                )
                .with_angle(*angle as f32),
            ),
            GateKind::Polygon { vertices } => Box::new(crate::gating::PolygonGate::new(
                x_idx,
                y_idx,
                vertices
                    .iter()
                    .map(|p| (p[0] as f32, p[1] as f32))
                    .collect(),
            )),
            GateKind::Quadrant { x, y, quad } => Box::new(QuadrantGate::new(
                x_idx,
                y_idx,
                *x as f32,
                *y as f32,
                quad.clone(),
            )),
            GateKind::Bilinear {
                x1,
                y1,
                x2,
                y2,
                below,
            } => Box::new(LineGate::new(
                x_idx, y_idx, *x1 as f32, *y1 as f32, *x2 as f32, *y2 as f32, *below,
            )),
            GateKind::Mask { indices } => Box::new(FileMaskGate {
                path: std::path::PathBuf::from("(imported)"),
                format: MaskFileFormat::Indices,
                mask: MaskGate::new(indices.clone()),
            }),
        }
    }

    /// Closed outline polygon in data coordinates (for rendering/filling).
    ///
    /// `x_range`/`y_range` are needed to clip the unbounded wedge gates.
    fn polygon(&self, x_range: (f64, f64), y_range: (f64, f64)) -> Vec<[f64; 2]> {
        let (x0, x1) = x_range;
        let (y0, y1) = y_range;
        match self {
            GateKind::Rect {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
            } => {
                let cx = (x_min + x_max) / 2.0;
                let cy = (y_min + y_max) / 2.0;
                let hw = (x_max - x_min) / 2.0;
                let hh = (y_max - y_min) / 2.0;
                let (s, c) = angle.sin_cos();
                let mut pts: Vec<[f64; 2]> = [[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]]
                    .iter()
                    .map(|[lx, ly]| [cx + lx * c - ly * s, cy + lx * s + ly * c])
                    .collect();
                pts.push(pts[0]);
                pts
            }
            GateKind::Ellipse {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
            } => {
                let cx = (x_min + x_max) / 2.0;
                let cy = (y_min + y_max) / 2.0;
                let rx = (x_max - x_min) / 2.0;
                let ry = (y_max - y_min) / 2.0;
                let (s, c) = angle.sin_cos();
                let n = 48;
                (0..=n)
                    .map(|i| {
                        let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
                        let lx = rx * a.cos();
                        let ly = ry * a.sin();
                        [cx + lx * c - ly * s, cy + lx * s + ly * c]
                    })
                    .collect()
            }
            GateKind::Polygon { vertices } => {
                let mut pts = vertices.clone();
                if pts.len() >= 3 {
                    pts.push(pts[0]);
                }
                pts
            }
            GateKind::Quadrant { x, y, quad } => {
                let rect = vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
                let half = match quad {
                    Quadrant::UpperLeft => vec![[x0, *y], [*x, *y], [*x, y1], [x0, y1]],
                    Quadrant::UpperRight => vec![[*x, *y], [x1, *y], [x1, y1], [*x, y1]],
                    Quadrant::LowerLeft => vec![[x0, y0], [*x, y0], [*x, *y], [x0, *y]],
                    Quadrant::LowerRight => vec![[*x, y0], [x1, y0], [x1, *y], [*x, *y]],
                };
                let _ = rect;
                let mut p = half;
                if p.len() >= 3 {
                    p.push(p[0]);
                }
                p
            }
            GateKind::Bilinear {
                x1: lx1,
                y1: ly1,
                x2: lx2,
                y2: ly2,
                below,
            } => {
                let (xa, xb) = effective_x_band(*lx1, *lx2, x0, x1);
                let ya = line_y_at(*lx1, *ly1, *lx2, *ly2, xa).clamp(y0, y1);
                let yb = line_y_at(*lx1, *ly1, *lx2, *ly2, xb).clamp(y0, y1);
                let y_edge = if *below { y0 } else { y1 };
                vec![[xa, ya], [xb, yb], [xb, y_edge], [xa, y_edge], [xa, ya]]
            }
            GateKind::Mask { .. } => Vec::new(),
        }
    }

    /// Divider lines (quadrant crosshair, bilinear crossing lines).
    fn divider_lines(&self, x_range: (f64, f64), y_range: (f64, f64)) -> Vec<Vec<[f64; 2]>> {
        let (x0, x1) = x_range;
        let (y0, y1) = y_range;
        match self {
            GateKind::Quadrant { x, y, .. } => {
                vec![vec![[*x, y0], [*x, y1]], vec![[x0, *y], [x1, *y]]]
            }
            GateKind::Bilinear {
                x1: lx1,
                y1: ly1,
                x2: lx2,
                y2: ly2,
                ..
            } => {
                vec![vec![[*lx1, *ly1], [*lx2, *ly2]]]
            }
            _ => Vec::new(),
        }
    }

    /// Short human-readable description for reports.
    fn summary(&self) -> String {
        match self {
            GateKind::Rect {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
            } => format!(
                "rect x[{x_min:.2},{x_max:.2}] y[{y_min:.2},{y_max:.2}] rot={:.1}deg",
                angle.to_degrees()
            ),
            GateKind::Ellipse {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
            } => format!(
                "ellipse x[{x_min:.2},{x_max:.2}] y[{y_min:.2},{y_max:.2}] rot={:.1}deg",
                angle.to_degrees()
            ),
            GateKind::Polygon { vertices } => {
                format!("polygon ({} vertices)", vertices.len())
            }
            GateKind::Quadrant { x, y, quad } => {
                format!("quadrant {quad:?} split x={x:.2} y={y:.2}")
            }
            GateKind::Bilinear {
                x1,
                y1,
                x2,
                y2,
                below,
            } => format!(
                "line ({x1:.2},{y1:.2})-({x2:.2},{y2:.2}) {}",
                if *below { "below" } else { "above" }
            ),
            GateKind::Mask { indices } => format!("mask ({} events)", indices.len()),
        }
    }

    /// Centre of the gate, used as the pivot for rotate / scale.
    fn center(&self) -> [f64; 2] {
        match self {
            GateKind::Rect {
                x_min,
                x_max,
                y_min,
                y_max,
                ..
            }
            | GateKind::Ellipse {
                x_min,
                x_max,
                y_min,
                y_max,
                ..
            } => [(x_min + x_max) / 2.0, (y_min + y_max) / 2.0],
            GateKind::Quadrant { x, y, .. } => [*x, *y],
            GateKind::Bilinear { x1, y1, x2, y2, .. } => [(x1 + x2) / 2.0, (y1 + y2) / 2.0],
            GateKind::Polygon { vertices, .. } => centroid(vertices),
            GateKind::Mask { .. } => [0.0, 0.0],
        }
    }

    /// The split point (crosshair) shared by the sibling wedges of a quadrant
    /// gate, if this gate is a quadrant.
    fn quadrant_split(&self) -> Option<[f64; 2]> {
        match self {
            GateKind::Quadrant { x, y, .. } => Some([*x, *y]),
            _ => None,
        }
    }

    /// Translate the whole gate by `(dx, dy)`.
    fn translate(&mut self, dx: f64, dy: f64) {
        match self {
            GateKind::Rect {
                x_min,
                x_max,
                y_min,
                y_max,
                ..
            }
            | GateKind::Ellipse {
                x_min,
                x_max,
                y_min,
                y_max,
                ..
            } => {
                *x_min += dx;
                *x_max += dx;
                *y_min += dy;
                *y_max += dy;
            }
            GateKind::Quadrant { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            GateKind::Bilinear { x1, y1, x2, y2, .. } => {
                *x1 += dx;
                *x2 += dx;
                *y1 += dy;
                *y2 += dy;
            }
            GateKind::Polygon { vertices, .. } => {
                for v in vertices {
                    v[0] += dx;
                    v[1] += dy;
                }
            }
            GateKind::Mask { .. } => {}
        }
    }

    /// Rotate the gate about `center` by `delta` radians.
    fn rotate_about(&mut self, center: [f64; 2], delta: f64) {
        match self {
            GateKind::Rect { angle, .. } | GateKind::Ellipse { angle, .. } => *angle += delta,
            // Quadrants are defined by axis-aligned splits; rotation does not apply.
            GateKind::Quadrant { .. } => {}
            GateKind::Bilinear { x1, y1, x2, y2, .. } => {
                let p1 = rotate_point([*x1, *y1], center, delta);
                let p2 = rotate_point([*x2, *y2], center, delta);
                *x1 = p1[0];
                *y1 = p1[1];
                *x2 = p2[0];
                *y2 = p2[1];
            }
            GateKind::Polygon { vertices, .. } => {
                for v in vertices {
                    *v = rotate_point(*v, center, delta);
                }
            }
            GateKind::Mask { .. } => {}
        }
    }

    /// Scale the gate about `center` by `factor`.
    fn scale_about(&mut self, center: [f64; 2], factor: f64) {
        let f = factor.abs().max(1e-6);
        match self {
            GateKind::Rect {
                x_min,
                x_max,
                y_min,
                y_max,
                ..
            }
            | GateKind::Ellipse {
                x_min,
                x_max,
                y_min,
                y_max,
                ..
            } => {
                *x_min = center[0] + (*x_min - center[0]) * f;
                *x_max = center[0] + (*x_max - center[0]) * f;
                *y_min = center[1] + (*y_min - center[1]) * f;
                *y_max = center[1] + (*y_max - center[1]) * f;
            }
            GateKind::Quadrant { x, y, .. } => {
                *x = center[0] + (*x - center[0]) * f;
                *y = center[1] + (*y - center[1]) * f;
            }
            GateKind::Bilinear { x1, y1, x2, y2, .. } => {
                *x1 = center[0] + (*x1 - center[0]) * f;
                *y1 = center[1] + (*y1 - center[1]) * f;
                *x2 = center[0] + (*x2 - center[0]) * f;
                *y2 = center[1] + (*y2 - center[1]) * f;
            }
            GateKind::Polygon { vertices, .. } => {
                for v in vertices {
                    v[0] = center[0] + (v[0] - center[0]) * f;
                    v[1] = center[1] + (v[1] - center[1]) * f;
                }
            }
            GateKind::Mask { .. } => {}
        }
    }

    /// Draggable handles for interactive manipulation, in data coordinates.
    fn handles(&self) -> Vec<([f64; 2], HandleId)> {
        match self {
            GateKind::Rect {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
            } => {
                let cx = (x_min + x_max) / 2.0;
                let cy = (y_min + y_max) / 2.0;
                let hw = (x_max - x_min) / 2.0;
                let hh = (y_max - y_min) / 2.0;
                let (s, c) = angle.sin_cos();
                let world = |lx: f64, ly: f64| [cx + lx * c - ly * s, cy + lx * s + ly * c];
                let margin = hh.abs().max(hw.abs()) * 0.15 + 1e-9;
                vec![
                    (world(-hw, -hh), HandleId::Corner(0)),
                    (world(hw, -hh), HandleId::Corner(1)),
                    (world(hw, hh), HandleId::Corner(2)),
                    (world(-hw, hh), HandleId::Corner(3)),
                    (world(0.0, hh + margin), HandleId::Rotate),
                ]
            }
            GateKind::Ellipse {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
            } => {
                let cx = (x_min + x_max) / 2.0;
                let cy = (y_min + y_max) / 2.0;
                let hw = (x_max - x_min) / 2.0;
                let hh = (y_max - y_min) / 2.0;
                let _diag = (hw * hw + hh * hh).sqrt().max(1e-9);
                let (s, c) = angle.sin_cos();
                let world = |lx: f64, ly: f64| [cx + lx * c - ly * s, cy + lx * s + ly * c];
                let margin = hh.abs().max(hw.abs()) * 0.15 + 1e-9;
                let inv_sqrt2 = 1.0 / 2.0f64.sqrt();
                // Corner handles sit ON the ellipse at 45-degree points (easy to grab).
                let mut out: Vec<([f64; 2], HandleId)> = vec![
                    (world(hw * inv_sqrt2, hh * inv_sqrt2), HandleId::Corner(0)),
                    (world(-hw * inv_sqrt2, hh * inv_sqrt2), HandleId::Corner(1)),
                    (world(-hw * inv_sqrt2, -hh * inv_sqrt2), HandleId::Corner(2)),
                    (world(hw * inv_sqrt2, -hh * inv_sqrt2), HandleId::Corner(3)),
                ];
                // Edge handles for single-axis stretch (left, top, right, bottom).
                out.push((world(hw, 0.0), HandleId::Edge(0)));
                out.push((world(0.0, hh), HandleId::Edge(1)));
                out.push((world(-hw, 0.0), HandleId::Edge(2)));
                out.push((world(0.0, -hh), HandleId::Edge(3)));
                // Rotation handle above the ellipse.
                out.push((world(0.0, hh + margin), HandleId::Rotate));
                out
            }
            GateKind::Polygon { vertices, .. } => {
                let c = centroid(vertices);
                let max_y = vertices
                    .iter()
                    .map(|p| p[1])
                    .fold(f64::NEG_INFINITY, f64::max);
                let span = (max_y - c[1]).abs().max(1e-9);
                let mut out: Vec<([f64; 2], HandleId)> = vertices
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (*p, HandleId::Vertex(i)))
                    .collect();
                out.push(([c[0], max_y + span * 0.2], HandleId::Rotate));
                out
            }
            GateKind::Bilinear { x1, y1, x2, y2, .. } => vec![
                ([*x1, *y1], HandleId::Vertex(0)),
                ([*x2, *y2], HandleId::Vertex(1)),
                ([(x1 + x2) / 2.0, (y1 + y2) / 2.0], HandleId::Center),
            ],
            GateKind::Quadrant { x, y, .. } => vec![([*x, *y], HandleId::Center)],
            GateKind::Mask { .. } => Vec::new(),
        }
    }

    /// Point-in-gate test (used to decide "move" vs "create" on drag).
    fn contains_point(&self, p: [f64; 2]) -> bool {
        let (x, y) = (p[0], p[1]);
        match self {
            GateKind::Rect {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
            } => {
                let cx = (x_min + x_max) / 2.0;
                let cy = (y_min + y_max) / 2.0;
                let hw = (x_max - x_min) / 2.0;
                let hh = (y_max - y_min) / 2.0;
                let (s, c) = angle.sin_cos();
                let lx = (x - cx) * c + (y - cy) * s;
                let ly = -(x - cx) * s + (y - cy) * c;
                lx.abs() <= hw && ly.abs() <= hh
            }
            GateKind::Ellipse {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
            } => {
                let cx = (x_min + x_max) / 2.0;
                let cy = (y_min + y_max) / 2.0;
                let rx = (x_max - x_min) / 2.0;
                let ry = (y_max - y_min) / 2.0;
                let (s, c) = angle.sin_cos();
                let lx = (x - cx) * c + (y - cy) * s;
                let ly = -(x - cx) * s + (y - cy) * c;
                let ex = if rx.abs() < 1e-12 {
                    f64::INFINITY
                } else {
                    lx / rx
                };
                let ey = if ry.abs() < 1e-12 {
                    f64::INFINITY
                } else {
                    ly / ry
                };
                ex * ex + ey * ey <= 1.0
            }
            GateKind::Quadrant { x: sx, y: sy, quad } => match quad {
                Quadrant::UpperLeft => x <= *sx && y >= *sy,
                Quadrant::UpperRight => x >= *sx && y >= *sy,
                Quadrant::LowerLeft => x <= *sx && y <= *sy,
                Quadrant::LowerRight => x >= *sx && y <= *sy,
            },
            GateKind::Bilinear { x1, y1, x2, y2, .. } => {
                // Treat a click near the line as a grab on the gate body.
                let seg2 = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).max(1e-12);
                let t = (((x - x1) * (x2 - x1) + (y - y1) * (y2 - y1)) / seg2).clamp(0.0, 1.0);
                let px = x1 + t * (x2 - x1);
                let py = y1 + t * (y2 - y1);
                let tol = (seg2.sqrt() * 0.04).max(1e-9);
                ((x - px).powi(2) + (y - py).powi(2)).sqrt() <= tol
            }
            GateKind::Polygon { vertices, .. } => point_in_polygon([x, y], vertices),
            GateKind::Mask { .. } => false,
        }
    }
}

/// Centroid of a set of vertices (falls back to the first point).
fn centroid(vertices: &[[f64; 2]]) -> [f64; 2] {
    if vertices.is_empty() {
        return [0.0, 0.0];
    }
    let n = vertices.len() as f64;
    let sx: f64 = vertices.iter().map(|p| p[0]).sum();
    let sy: f64 = vertices.iter().map(|p| p[1]).sum();
    [sx / n, sy / n]
}

/// A regular `n`-gon inscribed in the ellipse `(rx, ry)` centred on `(cx, cy)`.
fn regular_polygon(cx: f64, cy: f64, rx: f64, ry: f64, n: usize) -> Vec<[f64; 2]> {
    let n = n.max(3);
    (0..n)
        .map(|i| {
            let a = -std::f64::consts::FRAC_PI_2 + 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            [cx + rx * a.cos(), cy + ry * a.sin()]
        })
        .collect()
}

fn rotate_point(p: [f64; 2], center: [f64; 2], delta: f64) -> [f64; 2] {
    let (s, c) = delta.sin_cos();
    let dx = p[0] - center[0];
    let dy = p[1] - center[1];
    [center[0] + dx * c - dy * s, center[1] + dx * s + dy * c]
}

/// Paints a single line of text centred on `center` and rotated by `angle`
/// radians clockwise. Used to preview the layout editor's axis-label styling.
fn paint_label(
    painter: &egui::Painter,
    center: egui::Pos2,
    text: &str,
    size: f32,
    color: egui::Color32,
    family: egui::FontFamily,
    angle: f32,
) {
    let font = egui::FontId::new(size.max(1.0), family);
    let galley = painter.layout_no_wrap(text.to_string(), font, color);
    let half = galley.size() / 2.0;
    // Offset of the galley centre from its top-left, rotated about the top-left
    // (which is the pivot egui rotates around).
    let (s, c) = angle.sin_cos();
    let off = egui::vec2(half.x * c - half.y * s, half.x * s + half.y * c);
    let shape = egui::epaint::TextShape::new(center - off, galley, color).with_angle(angle);
    painter.add(egui::Shape::Text(shape));
}

/// A single-line editor for a layout label's custom text. Blank means "use the
/// parameter name".
fn label_text_edit(ui: &mut egui::Ui, id: &str, style: &mut LabelStyle) {
    let mut text = style.text.clone().unwrap_or_default();
    if ui
        .add(
            egui::TextEdit::singleline(&mut text)
                .id_salt(id)
                .hint_text("(parameter)")
                .desired_width(110.0),
        )
        .changed()
    {
        style.text = if text.trim().is_empty() {
            None
        } else {
            Some(text)
        };
    }
}

/// Draws a histogram-bar preview for a layout-editor panel in histogram
/// view. `pts` holds normalised `[0, 1]` x values (one per event, subsampled
/// when the panel was created); bins are drawn left to right.
fn draw_preview_histogram(
    painter: &egui::Painter,
    area: egui::Rect,
    pts: &[[f32; 2]],
    color: egui::Color32,
) {
    if pts.is_empty() {
        return;
    }
    const NBINS: usize = 8;
    let n = NBINS as f32;
    let bin_w = area.width() / n;
    let mut counts = [0u32; NBINS];
    for p in pts {
        let i = (p[0].clamp(0.0, 1.0) * n) as usize;
        counts[i.min(NBINS - 1)] += 1;
    }
    let max = counts.iter().copied().max().unwrap_or(1).max(1) as f32;
    let bar_w = bin_w * 0.8;
    for (i, &c) in counts.iter().enumerate() {
        let bh = area.height() * 0.9 * (c as f32 / max);
        if bh < 0.5 {
            continue;
        }
        let x = area.left() + i as f32 * bin_w + (bin_w - bar_w) / 2.0;
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, area.bottom() - bh), egui::vec2(bar_w, bh)),
            0.0_f32,
            color,
        );
    }
}

/// Font-family picker for a layout label.
fn label_font_combo(ui: &mut egui::Ui, id: &str, font: &mut LabelFont) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(font.label())
        .width(72.0)
        .show_ui(ui, |ui| {
            for f in LabelFont::ALL {
                ui.selectable_value(font, f, f.label());
            }
        });
}

fn point_in_polygon(p: [f64; 2], verts: &[[f64; 2]]) -> bool {
    if verts.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = verts.len() - 1;
    for i in 0..verts.len() {
        let (xi, yi) = (verts[i][0], verts[i][1]);
        let (xj, yj) = (verts[j][0], verts[j][1]);
        if ((yi > p[1]) != (yj > p[1])) && (p[0] < (xj - xi) * (p[1] - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Applies a manipulation to a snapshot of a gate, producing the new geometry.
fn apply_manipulation(
    snapshot: &GateKind,
    target: &ManipTarget,
    start: [f64; 2],
    cur: [f64; 2],
) -> GateKind {
    let mut g = snapshot.clone();
    let center = snapshot.center();
    match target {
        ManipTarget::Move => g.translate(cur[0] - start[0], cur[1] - start[1]),
        ManipTarget::Handle(HandleId::Center) => g.translate(cur[0] - start[0], cur[1] - start[1]),
        ManipTarget::Handle(HandleId::Rotate) => {
            let a0 = (start[1] - center[1]).atan2(start[0] - center[0]);
            let a1 = (cur[1] - center[1]).atan2(cur[0] - center[0]);
            g.rotate_about(center, a1 - a0);
        }
        ManipTarget::Handle(HandleId::Corner(_)) => {
            let d0 = ((start[0] - center[0]).powi(2) + (start[1] - center[1]).powi(2)).sqrt();
            let d1 = ((cur[0] - center[0]).powi(2) + (cur[1] - center[1]).powi(2)).sqrt();
            if d0 > 1e-9 {
                g.scale_about(center, d1 / d0);
            }
        }
        ManipTarget::Handle(HandleId::Edge(k)) => {
            if let GateKind::Ellipse {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
                ..
            } = &mut g
            {
                let cx = (*x_min + *x_max) / 2.0;
                let cy = (*y_min + *y_max) / 2.0;
                let (s, c) = angle.sin_cos();
                // Transform start and cur into the ellipse's local (unrotated) frame.
                let lx0 = (start[0] - cx) * c + (start[1] - cy) * s;
                let ly0 = -(start[0] - cx) * s + (start[1] - cy) * c;
                let lx1 = (cur[0] - cx) * c + (cur[1] - cy) * s;
                let ly1 = -(cur[0] - cx) * s + (cur[1] - cy) * c;
                match k {
                    0 | 2 => {
                        // Horizontal edges: scale x-radius about centre.
                        let old_rx = (*x_max - *x_min) / 2.0;
                        let ratio = if lx0.abs() > 1e-9 {
                            lx1.abs() / lx0.abs()
                        } else {
                            1.0
                        };
                        let new_rx = old_rx * ratio.clamp(0.01, 100.0);
                        *x_min = cx - new_rx;
                        *x_max = cx + new_rx;
                    }
                    1 | 3 => {
                        // Vertical edges: scale y-radius about centre.
                        let old_ry = (*y_max - *y_min) / 2.0;
                        let ratio = if ly0.abs() > 1e-9 {
                            ly1.abs() / ly0.abs()
                        } else {
                            1.0
                        };
                        let new_ry = old_ry * ratio.clamp(0.01, 100.0);
                        *y_min = cy - new_ry;
                        *y_max = cy + new_ry;
                    }
                    _ => {}
                }
            }
        }
        ManipTarget::Handle(HandleId::Vertex(i)) => match &mut g {
            GateKind::Polygon { vertices, .. } => {
                if *i < vertices.len() {
                    vertices[*i] = cur;
                }
            }
            GateKind::Bilinear { x1, y1, x2, y2, .. } => match i {
                0 => {
                    *x1 = cur[0];
                    *y1 = cur[1];
                }
                1 => {
                    *x2 = cur[0];
                    *y2 = cur[1];
                }
                _ => {}
            },
            _ => {}
        },
    }
    g
}

/// Per-plot interactive editing state.
#[derive(Default, Clone)]
struct Interact {
    /// Creation drag rectangle (data coords), while a gate is being drawn.
    drag_start: Option<[f64; 2]>,
    drag_current: Option<[f64; 2]>,
    /// Active gate manipulation, while a handle or the gate body is dragged.
    manip: Option<ManipSession>,
}

/// A gate manipulation in progress. The gate geometry is snapshotted when the
/// drag begins so updates are always computed from the original shape.
#[derive(Clone)]
struct ManipSession {
    gate: usize,
    snapshot: GateKind,
    target: ManipTarget,
    start: [f64; 2],
}

/// A request produced by [`drive_interact`] for the caller to apply.
enum InteractOutcome {
    Create { start: [f64; 2], end: [f64; 2] },
    Update { gate: usize, kind: GateKind },
    Select(usize),
    Click([f64; 2]),
}

/// Topmost gate whose body contains `pos`, preferring the selected gate when
/// several overlap.
fn gate_at(gates: &[GateDef], pos: [f64; 2], selected: Option<usize>) -> Option<usize> {
    if let Some(i) = selected
        && gates.get(i).is_some_and(|g| g.kind.contains_point(pos))
    {
        return Some(i);
    }
    (0..gates.len())
        .rev()
        .find(|&i| gates[i].kind.contains_point(pos))
}

/// Nearest handle within `tol` (data units) of `pos`.
fn nearest_handle(kind: &GateKind, pos: [f64; 2], tol: f64) -> Option<HandleId> {
    let mut best: Option<(f64, HandleId)> = None;
    for (hp, id) in kind.handles() {
        let d = ((hp[0] - pos[0]).powi(2) + (hp[1] - pos[1]).powi(2)).sqrt();
        if d <= tol && best.as_ref().map(|(bd, _)| d < *bd).unwrap_or(true) {
            best = Some((d, id));
        }
    }
    best.map(|(_, id)| id)
}

/// Advances per-plot interaction given the raw [`PlotInteraction`] events.
///
/// Decides between creating a new gate (drag on empty space) and manipulating
/// the selected gate (drag on one of its handles, or on its body to move it),
/// then returns the resulting outcome for the caller to apply.
fn drive_interact(
    it: &mut Interact,
    inter: &PlotInteraction,
    gates: &[GateDef],
    selected: Option<usize>,
    new_type: NewGateType,
    tol: f64,
) -> Option<InteractOutcome> {
    // Keep the in-progress creation rectangle up to date.
    if it.drag_start.is_some()
        && let Some(p) = inter.pointer
    {
        it.drag_current = Some(p);
    }

    let mut outcome = None;

    // A drag just started: choose manipulation vs. creation.
    if let Some(start) = inter.started {
        let mut creating = true;
        // 1. A handle on the selected gate always wins.
        if let Some(gi) = selected
            && let Some(gate) = gates.get(gi)
            && let Some(h) = nearest_handle(&gate.kind, start, tol)
        {
            it.manip = Some(ManipSession {
                gate: gi,
                snapshot: gate.kind.clone(),
                target: ManipTarget::Handle(h),
                start,
            });
            creating = false;
        }
        // 2. Otherwise any gate body under the pointer can be dragged. This
        //    also selects an unselected gate, so users do not have to select a
        //    rectangle / ellipse before moving it.
        if creating && let Some(gi) = gate_at(gates, start, selected) {
            it.manip = Some(ManipSession {
                gate: gi,
                snapshot: gates[gi].kind.clone(),
                target: ManipTarget::Move,
                start,
            });
            creating = false;
        }
        if creating {
            it.drag_start = Some(start);
            it.drag_current = Some(start);
        }
    }

    if let Some(end) = inter.released {
        if it.manip.take().is_some() {
            // Manipulation finished; nothing further to do.
        } else if let Some(a) = it.drag_start.take() {
            let b = inter.pointer.or(it.drag_current).unwrap_or(end);
            outcome = Some(InteractOutcome::Create { start: a, end: b });
        }
        it.drag_start = None;
        it.drag_current = None;
    } else if let Some(m) = &it.manip
        && let Some(cur) = inter.pointer
    {
        outcome = Some(InteractOutcome::Update {
            gate: m.gate,
            kind: apply_manipulation(&m.snapshot, &m.target, m.start, cur),
        });
    }

    if let Some(p) = inter.clicked {
        // Clicking a gate body selects it; clicking empty space places a
        // quadrant / bilinear gate when that tool is active.
        if let Some(gi) = gate_at(gates, p, selected) {
            outcome = Some(InteractOutcome::Select(gi));
        } else if matches!(new_type, NewGateType::Quadrant | NewGateType::Bilinear) {
            outcome = Some(InteractOutcome::Click(p));
        }
    }

    outcome
}

/// A gate definition, shared across all loaded samples.
#[derive(Debug, Clone, PartialEq)]
struct GateDef {
    name: String,
    /// Parameter indices this gate was defined on.
    x_idx: usize,
    y_idx: usize,
    /// Parent gate (builds the hierarchy). Always `< this gate's index`.
    parent: Option<usize>,
    kind: GateKind,
    color: egui::Color32,
    /// The sample this gate was created on (None = applies to all).
    owner: Option<usize>,
    /// Whether this gate applies to every loaded sample, or only its owner.
    apply_to_all: bool,
}

impl GateDef {
    fn to_gate(&self) -> Box<dyn GatingGate> {
        self.kind.to_gate(self.x_idx, self.y_idx)
    }
}

/// A gate copied to the internal clipboard.
///
/// The geometry is stored as the project's serializable [`GateShape`] so a copy
/// can be pasted into another workspace (or saved as text) without depending on
/// the live GUI objects.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CopiedGate {
    name: String,
    x_idx: usize,
    y_idx: usize,
    parent: Option<usize>,
    color: [u8; 3],
    apply_to_all: bool,
    kind: GateShape,
}

/// Choose `base` when no gate uses it, otherwise `base 2`, `base 3`, ...
fn unique_gate_name(gates: &[GateDef], base: &str) -> String {
    if !gates.iter().any(|g| g.name == base) {
        return base.to_string();
    }
    for n in 2.. {
        let candidate = format!("{base} {n}");
        if !gates.iter().any(|g| g.name == candidate) {
            return candidate;
        }
    }
    unreachable!("name space is infinite")
}

/// Computes the event indices for every gate, following the parent hierarchy.
///
/// `sample_idx` is the index of the sample being computed for. `sample_apply_gates`
/// is the per-sample switch set when the file was loaded (or toggled by the user);
/// when it is false the shared gate set is skipped for that sample. A gate is
/// evaluated for a sample when either the gate is marked "apply to all" *and* the
/// sample has gating enabled, or the sample is the gate's owner. Gates with an
/// out-of-range parameter index simply yield an empty population.
fn compute_gate_populations(
    data: &ndarray::Array2<f32>,
    gates: &[GateDef],
    sample_idx: Option<usize>,
    sample_apply_gates: bool,
) -> Vec<Vec<usize>> {
    let (total, n_params) = data.dim();
    let mut result: Vec<Vec<usize>> = vec![Vec::new(); gates.len()];
    for (i, gate) in gates.iter().enumerate() {
        // A gate always applies to the sample it was created on. Otherwise it
        // applies only when it is marked "apply to all" and the sample has
        // gating enabled.
        let owner = gate.owner == sample_idx;
        let shared = gate.apply_to_all && sample_apply_gates;
        if !owner && !shared {
            continue;
        }
        if gate.x_idx >= n_params || gate.y_idx >= n_params {
            continue;
        }
        let gate_obj = gate.to_gate();
        let base: Vec<usize> = match gate.parent {
            Some(p) if p < i => result[p].clone(),
            _ => (0..total).collect(),
        };
        result[i] = if let GateKind::Mask { .. } = gate.kind {
            // Mask gates select events by index, not by value.
            gate_obj.filter(data)
        } else {
            base.into_iter()
                .filter(|&e| gate_obj.contains(data.row(e)))
                .collect()
        };
    }
    result
}

/// Percentages for a gate: relative to its parent population (when it has one)
/// and relative to the whole sample.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GatePercentages {
    /// Percentage of the parent population; `None` for a root gate.
    of_parent: Option<f64>,
    /// Percentage of all events in the sample.
    of_total: f64,
}

/// Computes both percentages for a gate population.
fn gate_percentages(count: usize, parent_count: Option<usize>, total: usize) -> GatePercentages {
    fn pct(part: usize, whole: usize) -> f64 {
        if whole == 0 {
            0.0
        } else {
            part as f64 / whole as f64 * 100.0
        }
    }
    GatePercentages {
        of_parent: parent_count.map(|pc| pct(count, pc)),
        of_total: pct(count, total),
    }
}

/// Compact label showing a gate's percentage(s): one denominator for root
/// gates, both parent and total for nested gates.
fn gate_percentage_label(count: usize, parent_count: Option<usize>, total: usize) -> String {
    let pcts = gate_percentages(count, parent_count, total);
    match pcts.of_parent {
        Some(of_parent) => format!("{of_parent:.1}% parent, {:.1}% total", pcts.of_total),
        None => format!("{:.1}% total", pcts.of_total),
    }
}

/// Children lists from the parent pointers (for the hierarchy view).
fn children_map(gates: &[GateDef]) -> Vec<Vec<usize>> {
    let mut children = vec![Vec::new(); gates.len()];
    for (i, gate) in gates.iter().enumerate() {
        if let Some(p) = gate.parent
            && p < gates.len()
        {
            children[p].push(i);
        }
    }
    children
}

/// The effective horizontal extent of a line gate. A (near-)vertical segment has
/// no meaningful width, so it falls back to the full axis span; this keeps the
/// gate non-empty and matches the renderer's fill.
fn effective_x_band(x1: f64, x2: f64, x_range0: f64, x_range1: f64) -> (f64, f64) {
    let (a, b) = if x1 <= x2 { (x1, x2) } else { (x2, x1) };
    if (b - a).abs() < 1e-9 {
        (x_range0.min(x_range1), x_range0.max(x_range1))
    } else {
        (a, b)
    }
}

/// Y value of the line through `(x1, y1)..(x2, y2)` at `x`.
fn line_y_at(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    let dx = x2 - x1;
    if dx.abs() < 1e-9 {
        (y1 + y2) / 2.0
    } else {
        y1 + (y2 - y1) * (x - x1) / dx
    }
}

fn gate_color(i: usize) -> egui::Color32 {
    const PALETTE: [(u8, u8, u8); 8] = [
        (255, 140, 0),   // orange
        (0, 170, 220),   // cyan
        (40, 190, 80),   // green
        (215, 60, 200),  // magenta
        (230, 200, 30),  // yellow
        (120, 120, 255), // periwinkle
        (255, 100, 140), // pink
        (200, 60, 40),   // red
    ];
    let (r, g, b) = PALETTE[i % PALETTE.len()];
    egui::Color32::from_rgb(r, g, b)
}

fn color_to_hex(c: egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

// ---------------------------------------------------------------------------
// Plot rendering
// ---------------------------------------------------------------------------

/// A gate drawn on top of a plot, in data coordinates.
struct GateVisual {
    fill: Vec<[f64; 2]>,
    lines: Vec<Vec<[f64; 2]>>,
    color: egui::Color32,
    selected: bool,
    /// Outline width in points, from the view settings.
    line_width: f32,
    /// Optional median-cross overlay in data coordinates.
    median: Option<[f64; 2]>,
    /// Optional population label: text and anchor in data coordinates.
    label: Option<(String, [f64; 2])>,
    /// Draw this gate as a dashed backgated ancestor outline.
    backgate: bool,
}

/// Outline stroke for a gate overlay, honouring the configured line width.
fn gate_stroke(g: &GateVisual) -> egui::Stroke {
    let width = if g.selected {
        g.line_width + 1.0
    } else {
        g.line_width
    }
    .max(0.5);
    egui::Stroke::new(width, g.color)
}

/// Mean of the finite vertices, used to anchor population labels.
fn polygon_centroid(points: &[[f64; 2]]) -> Option<[f64; 2]> {
    let mut sum = [0.0f64, 0.0f64];
    let mut n = 0usize;
    for point in points {
        if point[0].is_finite() && point[1].is_finite() {
            sum[0] += point[0];
            sum[1] += point[1];
            n += 1;
        }
    }
    (n > 0).then(|| [sum[0] / n as f64, sum[1] / n as f64])
}

/// Everything needed to draw one plot.
struct PlotCfg {
    points: Vec<[f64; 2]>,
    x_range: (f64, f64),
    y_range: (f64, f64),
    x_scale: AxisScale,
    y_scale: AxisScale,
    point_color: egui::Color32,
    gradient: bool,
    view: ViewMode,
    density_map: crate::plot::DensityMap,
    /// Extra populations/samples drawn on the shared axes (V2).
    data_overlays: Vec<crate::plot::DataOverlay>,
    contour_levels: usize,
    gates: Vec<GateVisual>,
    /// Draggable manipulation handles for the selected gate (data coords).
    handles: Vec<([f64; 2], HandleId)>,
    x_label: String,
    y_label: String,
    interactive: bool,
    /// When true, the creation drag shows a line (for line/bilinear gates)
    /// instead of a rectangle.
    line_preview: bool,
    // -- V5 graph controls -------------------------------------------------
    smoothing: f32,
    resolution: usize,
    outlier_clip_percent: f64,
    large_dots: bool,
    grid: bool,
    medians: bool,
    show_event_numbers: bool,
    show_names_frequencies: bool,
    notation: crate::plot::TickNotation,
    show_axis_labels: bool,
    show_ticks: bool,
    show_numbers: bool,
    /// Draw adjunct X/Y histograms along the plot axes (V6).
    axis_histograms: bool,
    /// The display axes were swapped; gate editing is disabled.
    transposed: bool,
}

/// Interaction state passed into / out of [`show_plot`].
#[derive(Default, Clone)]
struct PlotInteraction {
    drag_start: Option<[f64; 2]>,
    drag_current: Option<[f64; 2]>,
    /// Set on the frame a drag begins (the press position, in data coords).
    started: Option<[f64; 2]>,
    /// Set on the frame a drag ends.
    released: Option<[f64; 2]>,
    /// Current pointer position (data coords) whenever the pointer is over the plot.
    pointer: Option<[f64; 2]>,
    clicked: Option<[f64; 2]>,
}

fn normalize_span(a: f64, b: f64) -> (f64, f64) {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    if (hi - lo).abs() < f64::EPSILON {
        (lo - 1.0, hi + 1.0)
    } else {
        (lo, hi)
    }
}

/// Draws a small horizontal gradient bar for a density colour map (V1).
fn density_legend_ui(ui: &mut egui::Ui, map: &crate::plot::DensityMap) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(110.0, 10.0), egui::Sense::hover());
    let painter = ui.painter();
    const STEPS: usize = 32;
    for i in 0..STEPS {
        let t0 = i as f64 / STEPS as f64;
        let t1 = (i + 1) as f64 / STEPS as f64;
        let (r, g, b) = crate::plot::density_colour(map, t0);
        let color = egui::Color32::from_rgb(
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8,
        );
        let x0 = rect.left() + rect.width() * t0 as f32;
        let x1 = rect.left() + rect.width() * t1 as f32;
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom())),
            0.0,
            color,
        );
    }
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(120)),
    );
}

/// Draws a single plot (main view, detached population window, or preview).
fn show_plot(ui: &mut egui::Ui, id: &str, cfg: &PlotCfg, inter: &mut PlotInteraction) {
    // Size the plot to the available UI space instead of a fixed size, so it
    // never overflows its panel and gets clipped by the panels around it (the
    // plot used to be laid out as a square of the full available width, which
    // pushed it past the bottom of the panel).
    let avail = ui.available_rect_before_wrap();
    // Transform the axis ranges into plot space.
    let xr = cfg.x_range;
    let yr = cfg.y_range;
    let tx = |v: f64| cfg.x_scale.transform(v).unwrap_or(0.0);
    let ty = |v: f64| cfg.y_scale.transform(v).unwrap_or(0.0);
    let (bx0, bx1) = normalize_span(tx(xr.0), tx(xr.1));
    let (by0, by1) = if matches!(cfg.view, ViewMode::Histogram) {
        (0.0, 1.0) // placeholder, overridden below after histogram computation
    } else {
        normalize_span(ty(yr.0), ty(yr.1))
    };

    // Transform all points up-front.
    let tpoints: Vec<[f64; 2]> = cfg
        .points
        .iter()
        .filter_map(|p| Some([cfg.x_scale.transform(p[0])?, cfg.y_scale.transform(p[1])?]))
        .collect();

    let use_density_view = matches!(cfg.view, ViewMode::Pseudocolor | ViewMode::Zebra);
    let need_density = cfg.gradient || use_density_view || matches!(cfg.view, ViewMode::Contour);
    let resolution = cfg.resolution.clamp(16, 512);
    let mut grid =
        need_density.then(|| density_grid(&tpoints, bx0, bx1, by0, by1, resolution, resolution));
    if let Some(g) = grid.as_mut() {
        crate::plot::smooth_density_grid(g, cfg.smoothing);
    }
    let densities = grid.as_ref().map(|g| {
        let dens = point_densities(&tpoints, g);
        let max_d = crate::plot::clipped_density_max(&dens, cfg.outlier_clip_percent);
        (max_d, dens)
    });

    // Bucket points by density (gradient / density-map views) or keep a single colour.
    let mut buckets: Vec<(egui::Color32, Vec<[f64; 2]>)> = Vec::new();
    if matches!(
        cfg.view,
        ViewMode::Dots | ViewMode::Pseudocolor | ViewMode::Zebra
    ) {
        if use_density_view {
            const NBINS: usize = 32;
            let mut bins: Vec<Vec<[f64; 2]>> = vec![Vec::new(); NBINS];
            if let (Some(_g), Some((max_d, dens))) = (&grid, &densities) {
                for (i, p) in tpoints.iter().enumerate() {
                    let t = if *max_d > 0.0 {
                        (dens[i] / max_d).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let b = ((t * (NBINS - 1) as f64).round() as usize).min(NBINS - 1);
                    bins[b].push(*p);
                }
            }
            for (b, pts) in bins.into_iter().enumerate() {
                if pts.is_empty() {
                    continue;
                }
                let (r, g, bl) =
                    crate::plot::density_colour(&cfg.density_map, b as f64 / (NBINS - 1) as f64);
                buckets.push((
                    egui::Color32::from_rgb(
                        (r * 255.0).round() as u8,
                        (g * 255.0).round() as u8,
                        (bl * 255.0).round() as u8,
                    ),
                    pts,
                ));
            }
        } else if cfg.gradient {
            const NBINS: usize = 16;
            let mut bins: Vec<Vec<[f64; 2]>> = vec![Vec::new(); NBINS];
            if let (Some(_g), Some((max_d, dens))) = (&grid, &densities) {
                for (i, p) in tpoints.iter().enumerate() {
                    let t = if *max_d > 0.0 {
                        (dens[i] / max_d).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let b = ((t * (NBINS - 1) as f64).round() as usize).min(NBINS - 1);
                    bins[b].push(*p);
                }
            }
            for (b, pts) in bins.into_iter().enumerate() {
                if pts.is_empty() {
                    continue;
                }
                let (r, g, bl) = density_rgb(b as f64 / (NBINS - 1) as f64);
                buckets.push((egui::Color32::from_rgb(r, g, bl), pts));
            }
        } else {
            buckets.push((cfg.point_color, tpoints.clone()));
        }
    }

    // Contour segments.
    let mut contour_paths: Vec<[[f64; 2]; 2]> = Vec::new();
    if matches!(cfg.view, ViewMode::Contour)
        && let Some(g) = grid.as_ref()
    {
        let max_d = crate::plot::clipped_density_max(&g.values, cfg.outlier_clip_percent);
        for level in crate::plot::contour_levels(max_d, cfg.contour_levels) {
            contour_paths.extend(crate::plot::contour_segments(g, level));
        }
    }
    let contour_color = egui::Color32::from_rgb(30, 110, 130);

    // Histogram bars (for Histogram view mode).
    let (hist_bars, hist_max_count, hist_bin_w) = if matches!(cfg.view, ViewMode::Histogram) {
        let xs: Vec<f64> = cfg.points.iter().map(|p| p[0]).collect();
        let (x0, x1) = cfg.x_range;
        // One bin per horizontal pixel makes the histogram read as a
        // smooth curve rather than a handful of blocky bars.
        let plot_px = avail.width().min(avail.height()).max(80.0);
        let bin_count = ((plot_px - 60.0).max(50.0) as usize).clamp(50, 2000);
        let bin_w = (x1 - x0) / bin_count as f64;
        let mut counts = vec![0usize; bin_count];
        for &x in &xs {
            if !x.is_finite() || x < x0 || x > x1 {
                continue;
            }
            let bi = ((x - x0) / bin_w) as usize;
            let bi = bi.min(bin_count - 1);
            counts[bi] += 1;
        }
        let max_count = counts.iter().max().copied().unwrap_or(1).max(1);
        let bars: Vec<Bar> = counts
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                let center = x0 + (i as f64 + 0.5) * bin_w;
                Bar::new(center, c as f64)
            })
            .collect();
        (Some(bars), max_count as f64, bin_w)
    } else {
        (None, 0.0, 0.0)
    };

    // Override y-range for histogram: counts from 0 to max count.
    let (by0, by1) = if matches!(cfg.view, ViewMode::Histogram) {
        (0.0, hist_max_count)
    } else {
        (by0, by1)
    };

    // Size the plot to the available UI space instead of a fixed size, so it
    // never overflows its panel and gets clipped by the panels around it (the
    // plot used to be laid out as a square of the full available width, which
    // pushed it past the bottom of the panel).
    let axes_visible = cfg.show_axis_labels || cfg.show_ticks || cfg.show_numbers;
    let x_axis_label = if cfg.show_axis_labels {
        cfg.x_label.as_str()
    } else {
        ""
    };
    let y_axis_label = if cfg.show_axis_labels {
        cfg.y_label.as_str()
    } else {
        ""
    };
    Plot::new(id)
        .view_aspect(1.0)
        .width(avail.width())
        .height(avail.height())
        .allow_boxed_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .show_axes(egui::Vec2b::new(axes_visible, axes_visible))
        .show_grid(egui::Vec2b::new(cfg.grid, cfg.grid))
        .x_axis_label(x_axis_label)
        .y_axis_label(y_axis_label)
        // Keep the rotated Y label clear of the tick numbers; egui_plot packs
        // them tightly together at the default width.
        .y_axis_min_width(72.0)
        .x_axis_formatter(|mark, _range| {
            if cfg.show_numbers {
                crate::plot::format_tick_with(mark.value, &cfg.x_scale, cfg.notation)
            } else {
                String::new()
            }
        })
        .y_axis_formatter(|mark, _range| {
            if cfg.show_numbers {
                crate::plot::format_tick_with(mark.value, &cfg.y_scale, cfg.notation)
            } else {
                String::new()
            }
        })
        .show(ui, |plot_ui| {
            plot_ui.set_plot_bounds(PlotBounds::from_min_max([bx0, by0], [bx1, by1]));

            // Global median lines (V5).
            if cfg.medians && !matches!(cfg.view, ViewMode::Histogram) {
                let xs: Vec<f64> = cfg.points.iter().map(|p| p[0]).collect();
                let ys: Vec<f64> = cfg.points.iter().map(|p| p[1]).collect();
                let median_color = cfg.point_color.gamma_multiply(0.75);
                if let Some(mx) = crate::plot::median_of(&xs)
                    .and_then(|v| cfg.x_scale.transform(v))
                    .filter(|v| *v >= bx0 && *v <= bx1)
                {
                    plot_ui.line(
                        Line::new(vec![[mx, by0], [mx, by1]])
                            .color(median_color)
                            .width(1.0_f32)
                            .style(egui_plot::LineStyle::dashed_dense()),
                    );
                }
                if let Some(my) = crate::plot::median_of(&ys)
                    .and_then(|v| cfg.y_scale.transform(v))
                    .filter(|v| *v >= by0 && *v <= by1)
                {
                    plot_ui.line(
                        Line::new(vec![[bx0, my], [bx1, my]])
                            .color(median_color)
                            .width(1.0_f32)
                            .style(egui_plot::LineStyle::dashed_dense()),
                    );
                }
            }

            // Event count annotation (V5).
            if cfg.show_event_numbers {
                let pos =
                    egui_plot::PlotPoint::new(bx0 + (bx1 - bx0) * 0.02, by1 - (by1 - by0) * 0.04);
                plot_ui.text(
                    egui_plot::Text::new(pos, format!("n = {}", cfg.points.len()))
                        .color(cfg.point_color)
                        .anchor(egui::Align2::LEFT_BOTTOM)
                        .allow_hover(false),
                );
            }

            // Adjunct axis histograms (V6): translucent strips inside the plot
            // area, along the top (X) and right (Y) edges. They use the same
            // transformed points and visible bounds as the scatter layer.
            if cfg.axis_histograms && !matches!(cfg.view, ViewMode::Histogram) {
                let bins = 48usize;
                let xs: Vec<f64> = tpoints.iter().map(|p| p[0]).collect();
                let ys: Vec<f64> = tpoints.iter().map(|p| p[1]).collect();
                let xcounts = crate::plot::smooth_counts(
                    &crate::plot::axis_histogram_counts(&xs, bx0, bx1, bins),
                    cfg.smoothing,
                );
                let ycounts = crate::plot::smooth_counts(
                    &crate::plot::axis_histogram_counts(&ys, by0, by1, bins),
                    cfg.smoothing,
                );
                let xmax = xcounts.iter().copied().max().unwrap_or(1).max(1) as f64;
                let ymax = ycounts.iter().copied().max().unwrap_or(1).max(1) as f64;
                let band = 0.16;
                let x_band = (by1 - by0) * band;
                let y_band = (bx1 - bx0) * band;
                let bin_w = (bx1 - bx0) / bins as f64;
                let bin_h = (by1 - by0) / bins as f64;
                let fill = egui::Color32::from_rgba_unmultiplied(
                    cfg.point_color.r(),
                    cfg.point_color.g(),
                    cfg.point_color.b(),
                    70,
                );
                let stroke = egui::Stroke::NONE;
                for (b, &count) in xcounts.iter().enumerate() {
                    if count == 0 {
                        continue;
                    }
                    let h = count as f64 / xmax * x_band;
                    let x0 = bx0 + b as f64 * bin_w;
                    let x1 = (x0 + bin_w).min(bx1);
                    let y0 = by1 - h;
                    plot_ui.polygon(
                        Polygon::new(vec![[x0, by1], [x1, by1], [x1, y0], [x0, y0]])
                            .fill_color(fill)
                            .stroke(stroke)
                            .allow_hover(false),
                    );
                }
                for (b, &count) in ycounts.iter().enumerate() {
                    if count == 0 {
                        continue;
                    }
                    let w = count as f64 / ymax * y_band;
                    let y0 = by0 + b as f64 * bin_h;
                    let y1 = (y0 + bin_h).min(by1);
                    let x0 = bx1 - w;
                    plot_ui.polygon(
                        Polygon::new(vec![[bx1, y0], [bx1, y1], [x0, y1], [x0, y0]])
                            .fill_color(fill)
                            .stroke(stroke)
                            .allow_hover(false),
                    );
                }
            }

            // Drag preview (created by the user while gating).
            if let (Some(a), Some(b)) = (inter.drag_start, inter.drag_current) {
                let a = [tx(a[0]), ty(a[1])];
                let b = [tx(b[0]), ty(b[1])];
                if cfg.line_preview {
                    plot_ui.line(
                        Line::new(vec![a, b])
                            .color(egui::Color32::from_rgb(0, 120, 200))
                            .width(2.0_f32),
                    );
                } else {
                    plot_ui.polygon(
                        Polygon::new(vec![
                            [a[0], a[1]],
                            [b[0], a[1]],
                            [b[0], b[1]],
                            [a[0], b[1]],
                            [a[0], a[1]],
                        ])
                        .fill_color(egui::Color32::from_rgba_unmultiplied(0, 150, 255, 60))
                        .stroke(egui::Stroke::new(
                            1.5_f32,
                            egui::Color32::from_rgb(0, 120, 200),
                        )),
                    );
                }
            }

            // Gates.
            for g in &cfg.gates {
                if g.backgate {
                    // Backgated ancestors: dashed outline, no fill, so they read
                    // as context rather than as a second active gate.
                    if g.fill.len() >= 2 {
                        let mut poly: Vec<[f64; 2]> = g
                            .fill
                            .iter()
                            .filter_map(|p| {
                                Some([cfg.x_scale.transform(p[0])?, cfg.y_scale.transform(p[1])?])
                            })
                            .collect();
                        if poly.len() >= 2 {
                            if let Some(first) = poly.first().copied() {
                                poly.push(first);
                            }
                            plot_ui.line(
                                Line::new(poly)
                                    .color(g.color.gamma_multiply(0.75))
                                    .width(g.line_width)
                                    .style(egui_plot::LineStyle::dashed_dense()),
                            );
                        }
                    }
                } else if g.fill.len() >= 3 {
                    let poly: Vec<[f64; 2]> = g
                        .fill
                        .iter()
                        .filter_map(|p| {
                            Some([cfg.x_scale.transform(p[0])?, cfg.y_scale.transform(p[1])?])
                        })
                        .collect();
                    if poly.len() >= 3 {
                        let fill = if g.selected {
                            egui::Color32::from_rgba_unmultiplied(
                                g.color.r(),
                                g.color.g(),
                                g.color.b(),
                                70,
                            )
                        } else {
                            egui::Color32::from_rgba_unmultiplied(
                                g.color.r(),
                                g.color.g(),
                                g.color.b(),
                                30,
                            )
                        };
                        plot_ui.polygon(Polygon::new(poly).fill_color(fill).stroke(gate_stroke(g)));
                    }
                }
                for line in &g.lines {
                    let pts: Vec<[f64; 2]> = line
                        .iter()
                        .filter_map(|p| {
                            Some([cfg.x_scale.transform(p[0])?, cfg.y_scale.transform(p[1])?])
                        })
                        .collect();
                    if pts.len() >= 2 {
                        let stroke = gate_stroke(g);
                        if g.backgate {
                            plot_ui.line(
                                Line::new(pts)
                                    .color(stroke.color.gamma_multiply(0.75))
                                    .width(stroke.width)
                                    .style(egui_plot::LineStyle::dashed_dense()),
                            );
                        } else {
                            plot_ui.line(Line::new(pts).stroke(stroke));
                        }
                    }
                }
                if let Some((text, [lx, ly])) = &g.label
                    && let (Some(px), Some(py)) =
                        (cfg.x_scale.transform(*lx), cfg.y_scale.transform(*ly))
                {
                    plot_ui.text(
                        egui_plot::Text::new(egui_plot::PlotPoint::new(px, py), text.as_str())
                            .color(g.color)
                            .anchor(egui::Align2::CENTER_CENTER)
                            .allow_hover(false),
                    );
                }
                if let Some([mx, my]) = g.median
                    && let (Some(px), Some(py)) =
                        (cfg.x_scale.transform(mx), cfg.y_scale.transform(my))
                {
                    let color = g.color.gamma_multiply(0.8);
                    let style = egui_plot::LineStyle::dashed_dense();
                    plot_ui.line(
                        Line::new(vec![[bx0, py], [bx1, py]])
                            .color(color)
                            .width(1.0_f32)
                            .style(style),
                    );
                    plot_ui.line(
                        Line::new(vec![[px, by0], [px, by1]])
                            .color(color)
                            .width(1.0_f32)
                            .style(style),
                    );
                }
            }

            // Manipulation handles for the selected gate.
            for (hp, id) in &cfg.handles {
                if let (Some(px), Some(py)) =
                    (cfg.x_scale.transform(hp[0]), cfg.y_scale.transform(hp[1]))
                {
                    let (color, shape) = match id {
                        HandleId::Rotate => (
                            egui::Color32::from_rgb(240, 210, 60),
                            egui_plot::MarkerShape::Diamond,
                        ),
                        HandleId::Center => (
                            egui::Color32::from_rgb(255, 255, 255),
                            egui_plot::MarkerShape::Cross,
                        ),
                        _ => (
                            egui::Color32::from_rgb(255, 255, 255),
                            egui_plot::MarkerShape::Square,
                        ),
                    };
                    plot_ui.points(
                        Points::new(vec![[px, py]])
                            .color(color)
                            .radius(4.0_f32)
                            .shape(shape)
                            .allow_hover(false),
                    );
                }
            }

            // Events (dots) or histogram bars.
            if matches!(cfg.view, ViewMode::Histogram) {
                if let Some(bars) = &hist_bars {
                    plot_ui.bar_chart(
                        BarChart::new(bars.clone())
                            .color(cfg.point_color)
                            .width(hist_bin_w * 0.95),
                    );
                }
            } else {
                let marker_radius = if cfg.large_dots { 4.0_f32 } else { 2.0_f32 };
                for (color, pts) in &buckets {
                    plot_ui.points(
                        Points::new(pts.clone())
                            .color(*color)
                            .radius(marker_radius)
                            .allow_hover(false),
                    );
                }
            }

            // Data overlays (V2): extra populations on the shared axes.
            if !matches!(cfg.view, ViewMode::Histogram) {
                for overlay in &cfg.data_overlays {
                    let (r, g, b) =
                        crate::plot::parse_hex_rgb(&overlay.color).unwrap_or((120, 120, 255));
                    let color = egui::Color32::from_rgba_unmultiplied(r, g, b, 190);
                    let mut pts: Vec<[f64; 2]> = Vec::new();
                    for p in &overlay.points {
                        if let (Some(px), Some(py)) =
                            (cfg.x_scale.transform(p[0]), cfg.y_scale.transform(p[1]))
                        {
                            pts.push([px, py]);
                        }
                    }
                    if pts.is_empty() {
                        continue;
                    }
                    match overlay.kind {
                        crate::plot::OverlayKind::Histogram => {
                            let bins = overlay.bins.max(2);
                            let mut counts = vec![0usize; bins];
                            for p in &pts {
                                let t = ((p[0] - bx0) / (bx1 - bx0)).clamp(0.0, 1.0);
                                let bi = ((t * (bins - 1) as f64).round() as usize).min(bins - 1);
                                counts[bi] += 1;
                            }
                            let max = counts.iter().copied().max().unwrap_or(0) as f64;
                            if max > 0.0 {
                                let mut line = Vec::new();
                                for (bi, &c) in counts.iter().enumerate() {
                                    let px = bx0 + (bx1 - bx0) * bi as f64 / (bins - 1) as f64;
                                    let py = by0 + (by1 - by0) * 0.35 * (c as f64 / max);
                                    line.push([px, py]);
                                }
                                plot_ui.line(Line::new(line).color(color).width(1.4_f32));
                            }
                        }
                        _ => {
                            let radius = if cfg.large_dots { 3.2_f32 } else { 1.6_f32 };
                            plot_ui.points(
                                Points::new(pts)
                                    .color(color)
                                    .radius(radius)
                                    .allow_hover(false),
                            );
                        }
                    }
                }
                // Overlay legend with optional frequencies (V5).
                if cfg.show_names_frequencies && !cfg.data_overlays.is_empty() {
                    let span_y = (by1 - by0).max(1e-9);
                    let mut ly = by1 - span_y * 0.04;
                    for overlay in &cfg.data_overlays {
                        if overlay.label.is_empty() {
                            continue;
                        }
                        let (r, g, b) =
                            crate::plot::parse_hex_rgb(&overlay.color).unwrap_or((120, 120, 255));
                        let label = if cfg.points.is_empty() {
                            overlay.label.clone()
                        } else {
                            format!(
                                "{} ({:.1}%)",
                                overlay.label,
                                overlay.points.len() as f64 / cfg.points.len() as f64 * 100.0
                            )
                        };
                        plot_ui.text(
                            egui_plot::Text::new(
                                egui_plot::PlotPoint::new(bx1 - (bx1 - bx0) * 0.02, ly),
                                label,
                            )
                            .color(egui::Color32::from_rgb(r, g, b))
                            .anchor(egui::Align2::RIGHT_TOP)
                            .allow_hover(false),
                        );
                        ly -= span_y * 0.05;
                    }
                }
            }

            // Contour lines.
            for seg in &contour_paths {
                plot_ui.line(
                    Line::new(vec![seg[0], seg[1]])
                        .color(contour_color)
                        .width(1.2_f32),
                );
            }

            // Interaction: report pointer position and drag/click events.
            if cfg.interactive {
                let resp = plot_ui.response();
                if let Some(pos) = plot_ui.pointer_coordinate() {
                    inter.pointer = Some([cfg.x_scale.inverse(pos.x), cfg.y_scale.inverse(pos.y)]);
                }

                if resp.drag_started() {
                    inter.started = inter.pointer;
                }
                if resp.drag_stopped() {
                    inter.released = inter.pointer.or(inter.started);
                }
                if resp.clicked() {
                    inter.clicked = inter.pointer;
                }
            }
        });

    // Set the mouse cursor based on which handle (if any) is hovered.
    if cfg.interactive
        && let Some(ptr) = inter.pointer
    {
        let span = (cfg.x_range.1 - cfg.x_range.0)
            .abs()
            .max((cfg.y_range.1 - cfg.y_range.0).abs())
            .max(1.0);
        let tol = 0.03 * span;
        if let Some((_, handle_id)) = cfg
            .handles
            .iter()
            .find(|(hp, _)| ((hp[0] - ptr[0]).powi(2) + (hp[1] - ptr[1]).powi(2)).sqrt() <= tol)
        {
            let cursor = match handle_id {
                HandleId::Corner(0) | HandleId::Corner(2) => egui::CursorIcon::ResizeNwSe,
                HandleId::Corner(1) | HandleId::Corner(3) => egui::CursorIcon::ResizeNeSw,
                HandleId::Edge(0) | HandleId::Edge(2) => egui::CursorIcon::ResizeHorizontal,
                HandleId::Edge(1) | HandleId::Edge(3) => egui::CursorIcon::ResizeVertical,
                HandleId::Rotate => egui::CursorIcon::Alias,
                HandleId::Vertex(_) | HandleId::Center => egui::CursorIcon::Grab,
                _ => egui::CursorIcon::Default,
            };
            ui.ctx().set_cursor_icon(cursor);
        }
    }
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

/// A loaded FCS sample.
struct SampleEntry {
    id: String,
    /// Full path to the source FCS file (used when saving a workspace).
    path: std::path::PathBuf,
    fcs: FcsFile,
    /// Uncompensated event data; `fcs.data` is derived from this by applying
    /// `compensation` (when present).
    raw_data: ndarray::Array2<f32>,
    /// Compensation matrix currently applied to `fcs.data`, if any.
    compensation: Option<CompensationMatrix>,
    /// Whether the shared gate set is applied to this sample. Controlled from
    /// the sample list and by the per-file prompt shown when a file is loaded
    /// after gates already exist.
    apply_gates: bool,
}

/// Which gate type new gates get when drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum NewGateType {
    #[default]
    Rect,
    Ellipse,
    Polygon,
    Quadrant,
    Bilinear,
}

impl NewGateType {
    fn label(self) -> &'static str {
        match self {
            NewGateType::Rect => "Rectangle",
            NewGateType::Ellipse => "Ellipse",
            NewGateType::Polygon => "Polygon",
            NewGateType::Quadrant => "Quadrant",
            NewGateType::Bilinear => "Bilinear",
        }
    }
    const ALL: [NewGateType; 5] = [
        NewGateType::Rect,
        NewGateType::Ellipse,
        NewGateType::Polygon,
        NewGateType::Quadrant,
        NewGateType::Bilinear,
    ];
}

/// Font family choices for layout axis labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabelFont {
    Sans,
    Serif,
    Mono,
}

impl LabelFont {
    const ALL: [LabelFont; 3] = [LabelFont::Sans, LabelFont::Serif, LabelFont::Mono];

    fn label(self) -> &'static str {
        match self {
            LabelFont::Sans => "Sans",
            LabelFont::Serif => "Serif",
            LabelFont::Mono => "Mono",
        }
    }

    /// CSS font-family used in SVG export.
    fn css(self) -> &'static str {
        match self {
            LabelFont::Sans => "sans-serif",
            LabelFont::Serif => "serif",
            LabelFont::Mono => "monospace",
        }
    }

    /// egui font family used for the live preview.
    fn egui(self) -> egui::FontFamily {
        match self {
            LabelFont::Mono => egui::FontFamily::Monospace,
            _ => egui::FontFamily::Proportional,
        }
    }
}

/// Styling for one axis label in the layout editor.
#[derive(Debug, Clone, PartialEq)]
struct LabelStyle {
    /// Custom text; `None` uses the parameter name.
    text: Option<String>,
    /// Font size (SVG units).
    size: f32,
    /// Rotation, in degrees (clockwise).
    rotation_deg: f32,
    font: LabelFont,
    color: egui::Color32,
}

impl LabelStyle {
    fn x_default() -> Self {
        Self {
            text: None,
            size: 11.0,
            rotation_deg: 0.0,
            font: LabelFont::Sans,
            color: egui::Color32::from_gray(90),
        }
    }

    fn y_default() -> Self {
        Self {
            text: None,
            size: 11.0,
            rotation_deg: -90.0,
            font: LabelFont::Sans,
            color: egui::Color32::from_gray(90),
        }
    }
}

/// One entry in the layout editor. Panels live on a free-form canvas and can be
/// moved / resized with the mouse (Photoshop-style).
#[derive(Clone, PartialEq)]
struct LayoutPanelSpec {
    sample: usize,
    x: usize,
    y: usize,
    view: ViewMode,
    /// Optional gate index: if set, the panel shows only that gated population.
    gate: Option<usize>,
    title: String,
    /// Font size (in SVG units) of the panel's title.
    title_size: f32,
    /// Position & size on the layout canvas, in canvas pixels.
    rect: egui::Rect,
    /// Normalised (0..1) preview points, cached when the panel is added.
    preview: Vec<[f32; 2]>,
    /// Styling for the X and Y axis labels.
    x_label: LabelStyle,
    y_label: LabelStyle,
}

/// An in-progress canvas manipulation (move or resize) of a layout panel.
#[derive(Clone, Copy)]
struct LayoutDrag {
    panel: usize,
    resize: bool,
    grab: egui::Vec2,
}

/// Detached-window state: an independently configurable view of one population.
#[derive(Clone)]
struct PopoutState {
    open: bool,
    gate: Option<usize>,
    x_param: Option<usize>,
    y_param: Option<usize>,
    view: ViewMode,
    x_scale: AxisScale,
    y_scale: AxisScale,
    point_color: egui::Color32,
    gradient: bool,
    /// Density colour map for pseudocolour / zebra views (V1).
    density_map: crate::plot::DensityMap,
    contour_levels: usize,
    x_range: (f64, f64),
    y_range: (f64, f64),
    new_gate_type: NewGateType,
    selected_gate: Option<usize>,
    interact: Interact,
}

impl Default for PopoutState {
    fn default() -> Self {
        Self {
            open: false,
            gate: None,
            x_param: None,
            y_param: None,
            view: ViewMode::Dots,
            x_scale: AxisScale::Linear,
            y_scale: AxisScale::Linear,
            point_color: egui::Color32::from_rgb(31, 119, 180),
            gradient: true,
            density_map: crate::plot::DensityMap::Rainbow,
            contour_levels: 8,
            x_range: (0.0, 1.0),
            y_range: (0.0, 1.0),
            new_gate_type: NewGateType::Rect,
            selected_gate: None,
            interact: Interact::default(),
        }
    }
}

/// Colour theme for the whole application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    fn label(self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::Light => "Light",
        }
    }
}

/// A snapshot of the editable application state for Undo / Redo.
///
/// Loaded samples (the FCS data) are deliberately excluded: they are inputs, not
/// edits, so undoing never unloads a file. Everything a user can change about the
/// gating, the view and the layout is captured here.
#[derive(Clone, PartialEq)]
struct UndoState {
    gates: Vec<GateDef>,
    selected_gate: Option<usize>,
    x_param: Option<usize>,
    y_param: Option<usize>,
    view_mode: ViewMode,
    x_scale: AxisScale,
    y_scale: AxisScale,
    point_color: egui::Color32,
    gradient: bool,
    density_map: crate::plot::DensityMap,
    contour_levels: usize,
    gate_line_width: f32,
    show_median_cross: bool,
    magnetic_snap: bool,
    magnetic_strength: f32,
    magnetic_radius_frac: f32,
    gate_labels: GateLabelSettings,
    x_range: (f64, f64),
    y_range: (f64, f64),
    layout_panels: Vec<LayoutPanelSpec>,
    layout_columns: usize,
    layout_selected: Option<usize>,
}

impl UndoState {
    /// Compares everything that counts as a *document* edit, ignoring the
    /// transient selection fields so that merely selecting a gate or panel does
    /// not create an undo step.
    fn same_document(&self, other: &Self) -> bool {
        self.gates == other.gates
            && self.x_param == other.x_param
            && self.y_param == other.y_param
            && self.view_mode == other.view_mode
            && self.x_scale == other.x_scale
            && self.y_scale == other.y_scale
            && self.point_color == other.point_color
            && self.gradient == other.gradient
            && self.density_map == other.density_map
            && self.contour_levels == other.contour_levels
            && self.gate_line_width == other.gate_line_width
            && self.show_median_cross == other.show_median_cross
            && self.magnetic_snap == other.magnetic_snap
            && self.magnetic_strength == other.magnetic_strength
            && self.magnetic_radius_frac == other.magnetic_radius_frac
            && self.gate_labels == other.gate_labels
            && self.x_range == other.x_range
            && self.y_range == other.y_range
            && self.layout_panels == other.layout_panels
            && self.layout_columns == other.layout_columns
    }
}

/// Population navigation actions available in graph windows (V7).
#[derive(Clone, Copy, PartialEq, Eq)]
enum PopNav {
    Parent,
    FirstChild,
    NextSibling,
}

/// Cycles an axis parameter index, wrapping at both ends.
fn cycle_param(current: Option<usize>, len: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let cur = match current {
        Some(i) if i < len => i as isize,
        _ if delta >= 0 => -1,
        _ => 0,
    };
    Some((cur + delta).rem_euclid(len as isize) as usize)
}

/// The egui application state.
pub struct FlowersApp {
    samples: Vec<SampleEntry>,
    current_sample: Option<usize>,
    x_param: Option<usize>,
    y_param: Option<usize>,

    gates: Vec<GateDef>,
    selected_gate: Option<usize>,
    /// Serialized [`CopiedGate`] for Ctrl+C / Ctrl+V.
    gate_clipboard: Option<String>,
    /// Cached per-gate event indices for the current sample.
    pop_indices: Vec<Vec<usize>>,
    /// Cached children lists.
    children: Vec<Vec<usize>>,

    // Plot configuration.
    view_mode: ViewMode,
    x_scale: AxisScale,
    y_scale: AxisScale,
    point_color: egui::Color32,
    gradient: bool,
    /// Density colour map for pseudocolour / zebra views (V1).
    density_map: crate::plot::DensityMap,
    /// Draw other populations of the current X/Y pair as data overlays (V2).
    overlay_populations: bool,
    // -- V5 graph controls --------------------------------------------------
    /// Swap the X and Y axes for display.
    transpose: bool,
    /// Density smoothing fraction in `[0, 1]`.
    plot_smoothing: f32,
    /// Density grid resolution in cells per axis.
    plot_resolution: usize,
    /// Top percentage of events saturated on the density scale.
    outlier_clip_percent: f64,
    /// Double the event marker radius.
    large_dots: bool,
    /// Draw a background grid.
    show_grid: bool,
    /// Draw median lines across the plot.
    show_medians: bool,
    /// Show the plotted event count.
    show_event_numbers: bool,
    /// Append frequencies to overlay names.
    show_names_frequencies: bool,
    /// Tick-number notation.
    tick_notation: crate::plot::TickNotation,
    /// Show X/Y axis labels.
    show_axis_labels: bool,
    /// Draw tick marks.
    show_ticks: bool,
    /// Draw tick numbers.
    show_numbers: bool,
    /// Draw adjunct X/Y histograms along the plot axes (V6).
    show_axis_histograms: bool,
    contour_levels: usize,
    gate_line_width: f32,
    show_median_cross: bool,
    magnetic_snap: bool,
    magnetic_strength: f32,
    magnetic_radius_frac: f32,
    /// Population/gate overlay label options.
    gate_labels: GateLabelSettings,
    /// Automatic axis-range rules (T5).
    autorange_rules: Vec<AutorangeRule>,
    x_range: (f64, f64),
    y_range: (f64, f64),
    x_data_range: (f64, f64),
    y_data_range: (f64, f64),

    /// Named statistic sets persisted with the analysis.
    statistic_sets: Vec<crate::statistics::StatisticSet>,

    // Statistics panel (S5).
    /// Whether the statistics panel window is open.
    stats_panel_open: bool,

    // 3D view (V3).
    /// Whether the 3D view window is open.
    view3d_open: bool,
    /// Camera + parameter mapping for the 3D view.
    view3d_spec: crate::plot3d::Spec3D,
    /// Number of frames written by "Export animation".
    view3d_frames: usize,

    // Cached scatter points for the current sample + parameters.
    plot_points: Vec<[f64; 2]>,

    // Gate creation & manipulation.
    new_gate_type: NewGateType,
    interact: Interact,

    // Detached population window.
    /// Detached graph windows, each with its own `PlotSpec`-style state (V7).
    popouts: Vec<PopoutState>,
    /// Graph window that owns the keyboard navigation shortcuts.
    active_popout: Option<usize>,

    // Layout editor.
    layout_panels: Vec<LayoutPanelSpec>,
    layout_columns: usize,
    layout_open: bool,
    layout_selected: Option<usize>,
    layout_drag: Option<LayoutDrag>,

    // Appearance / export.
    theme: Theme,
    export_scale: f32,

    // Compensation editor.
    /// Whether the compensation window is open.
    comp_open: bool,
    /// Matrix being edited (applied with the buttons in the window).
    comp_draft: Option<CompensationMatrix>,
    /// Status / validation message shown in the compensation window.
    comp_message: String,
    /// Whether the compensation node is shown in the population tree (C6).
    comp_node_visible: bool,

    // Custom lookup transform table (used by the "Lookup" axis scale).
    /// The active lookup table, if any.
    lookup_table: LookupTable,
    /// Source label / status for the current lookup table.
    lookup_source: Option<String>,
    /// Validation or error message from the last lookup edit.
    lookup_message: String,

    /// Whether the Table Editor window is open.
    table_open: bool,

    name_edit: String,
    status: String,
    /// Index of a newly loaded sample waiting for the "apply existing gates?"
    /// prompt. `None` when no prompt is pending.
    pending_apply_prompt: Option<usize>,

    // Quit confirmation.
    /// True while the "unsaved changes" quit confirmation popup is open.
    quit_confirm: bool,
    /// Set when the user explicitly chooses to close without saving, so the
    /// window-close handler lets the close through instead of prompting again.
    allow_close: bool,
    /// True when the current document differs from the last saved workspace.
    dirty: bool,

    // Undo / redo.
    /// Snapshots of [`UndoState`], oldest first. `history[history_idx]` is the
    /// currently-applied state.
    history: Vec<UndoState>,
    history_idx: usize,
    /// Set for one frame after an undo/redo so the state comparison at the end
    /// of the frame does not push a redundant entry.
    history_skip: bool,
}

impl Default for FlowersApp {
    fn default() -> Self {
        Self {
            samples: Vec::new(),
            current_sample: None,
            x_param: None,
            y_param: None,
            gates: Vec::new(),
            selected_gate: None,
            gate_clipboard: None,
            pop_indices: Vec::new(),
            children: Vec::new(),
            view_mode: ViewMode::Dots,
            x_scale: AxisScale::Linear,
            y_scale: AxisScale::Linear,
            point_color: egui::Color32::from_rgb(31, 119, 180),
            gradient: true,
            density_map: crate::plot::DensityMap::Rainbow,
            overlay_populations: false,
            transpose: false,
            plot_smoothing: 0.0,
            plot_resolution: 128,
            outlier_clip_percent: 0.0,
            large_dots: false,
            show_grid: false,
            show_medians: false,
            show_event_numbers: false,
            show_names_frequencies: false,
            tick_notation: crate::plot::TickNotation::Auto,
            show_axis_labels: true,
            show_ticks: true,
            show_numbers: true,
            show_axis_histograms: false,
            contour_levels: 8,
            gate_line_width: 1.5,
            show_median_cross: false,
            magnetic_snap: false,
            magnetic_strength: 0.6,
            magnetic_radius_frac: 0.04,
            gate_labels: GateLabelSettings::default(),
            autorange_rules: Vec::new(),
            x_range: (0.0, 1.0),
            y_range: (0.0, 1.0),
            x_data_range: (0.0, 1.0),
            y_data_range: (0.0, 1.0),
            statistic_sets: Vec::new(),
            stats_panel_open: false,
            view3d_open: false,
            view3d_spec: crate::plot3d::Spec3D::default(),
            view3d_frames: 24,
            plot_points: Vec::new(),
            new_gate_type: NewGateType::Rect,
            interact: Interact::default(),
            popouts: Vec::new(),
            active_popout: None,
            layout_panels: Vec::new(),
            layout_columns: 2,
            layout_open: false,
            layout_selected: None,
            layout_drag: None,
            theme: Theme::Dark,
            export_scale: 2.0,
            comp_open: false,
            comp_draft: None,
            comp_message: String::new(),
            comp_node_visible: true,
            lookup_table: LookupTable::default(),
            lookup_source: None,
            lookup_message: String::new(),
            table_open: false,
            name_edit: String::new(),
            status: "Ready — load an FCS file".to_string(),
            pending_apply_prompt: None,
            quit_confirm: false,
            allow_close: false,
            dirty: false,
            history: Vec::new(),
            history_idx: 0,
            history_skip: false,
        }
    }
}

/// Human-readable display name for a parameter.
fn param_display_name(fcs: &FcsFile, idx: usize) -> String {
    fcs.parameters
        .get(idx)
        .map(|p| {
            let desc = p.description.trim();
            let name = p.name.trim();
            if !desc.is_empty() && desc != name {
                desc.to_string()
            } else {
                name.to_string()
            }
        })
        .unwrap_or_else(|| format!("P{idx}"))
}

/// Chooses sensible default X/Y parameters for a newly loaded file.
///
/// Prefers **FSC-H** and **SSC-H** (falling back to FSC-A/SSC-A, then any FSC/
/// SSC channel), and otherwise uses the first two parameters. The two axes are
/// guaranteed to differ when the file has at least two parameters.
fn default_param_pair(fcs: &FcsFile) -> (usize, usize) {
    let n = fcs.parameters.len();
    if n == 0 {
        return (0, 0);
    }
    let find = |want: &str| {
        fcs.parameters.iter().position(|p| {
            p.name.trim().eq_ignore_ascii_case(want)
                || p.description.trim().eq_ignore_ascii_case(want)
        })
    };
    let x = find("FSC-H")
        .or_else(|| find("FSC-A"))
        .or_else(|| find("FSC"))
        .unwrap_or(0);
    let y = find("SSC-H")
        .or_else(|| find("SSC-A"))
        .or_else(|| find("SSC"))
        .unwrap_or(if n > 1 { 1 } else { 0 });
    let y = if y == x && n > 1 { (x + 1) % n } else { y };
    (x, y)
}

/// `$PnN` names in file order.
fn parameter_names(fcs: &FcsFile) -> Vec<String> {
    fcs.parameters.iter().map(|p| p.name.clone()).collect()
}

/// `$PnS` descriptions in file order.
fn parameter_descriptions(fcs: &FcsFile) -> Vec<String> {
    fcs.parameters
        .iter()
        .map(|p| p.description.clone())
        .collect()
}

/// Reads the acquisition spillover matrix from `$SPILLOVER` and turns it into
/// an applied compensation matrix (the inverse of the stored spillover).
fn acquisition_compensation(fcs: &FcsFile) -> Option<CompensationMatrix> {
    fcs.keywords
        .get("$SPILLOVER")
        .and_then(|text| CompensationMatrix::from_spillover_keyword(text).ok())
        .and_then(|m| m.inverted().ok())
}

/// Applies an acquisition-defined spillover matrix when the file carries one.
/// Returns the (possibly compensated) file, the matrix that was applied, and
/// the original uncompensated data.
fn with_acquisition_compensation(
    mut fcs: FcsFile,
) -> (FcsFile, Option<CompensationMatrix>, ndarray::Array2<f32>) {
    let raw_data = fcs.data.clone();
    let mut compensation = acquisition_compensation(&fcs);
    if let Some(matrix) = &compensation {
        let names = parameter_names(&fcs);
        let descriptions = parameter_descriptions(&fcs);
        match matrix.apply_to(&raw_data, &names, &descriptions) {
            Ok(data) => fcs.data = data,
            Err(_) => compensation = None,
        }
    }
    (fcs, compensation, raw_data)
}

/// Applies a stored matrix to a freshly parsed file, retaining the raw data so
/// compensation can later be removed.
fn with_compensation(
    mut fcs: FcsFile,
    matrix: Option<&CompensationMatrix>,
) -> (FcsFile, Option<CompensationMatrix>, ndarray::Array2<f32>) {
    let raw_data = fcs.data.clone();
    let Some(matrix) = matrix else {
        return (fcs, None, raw_data);
    };
    let names = parameter_names(&fcs);
    let descriptions = parameter_descriptions(&fcs);
    match matrix.apply_to(&raw_data, &names, &descriptions) {
        Ok(data) => {
            fcs.data = data;
            (fcs, Some(matrix.clone()), raw_data)
        }
        Err(_) => (fcs, None, raw_data),
    }
}

/// A sensible starting set of channels for the compensation editor: everything
/// that is not a scatter / time channel, falling back to all parameters.
fn default_compensation_channels(fcs: &FcsFile) -> Vec<String> {
    let is_scatter_or_time = |name: &str| {
        let n = name.trim().to_ascii_lowercase();
        n.contains("fsc")
            || n.contains("ssc")
            || n.contains("forward")
            || n.contains("side")
            || n == "time"
            || n.starts_with("time ")
    };
    let fluoro: Vec<String> = fcs
        .parameters
        .iter()
        .map(|p| p.name.clone())
        .filter(|n| !is_scatter_or_time(n))
        .collect();
    if fluoro.len() >= 2 {
        fluoro
    } else {
        parameter_names(fcs)
    }
}

/// Compact, precise formatting for editable axis-range fields.
///
/// Flow-cytometry axes are often on the order of 10^7, where a plain decimal is
/// hard to read and type. Large/small magnitudes are shown in scientific
/// notation (e.g. `5.300871e6`) and mid-range values in plain notation.
///
/// Both branches use Rust's shortest round-tripping float formatting, so the
/// displayed text parses back to exactly the same value — focusing a field and
/// clicking away never nudges the number.
/// Formats a number for axis-range display.
///
/// Values with magnitude >= 1e5 or < 1e-2 are shown in scientific notation
/// rounded to 3 significant figures (e.g. `3.45e6`, `1.00e-4`); mid-range
/// values are shown in plain notation. The displayed text round-trips exactly
/// when parsed back.
fn format_edit_num(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let abs = v.abs();
    if abs >= 1.0e5 || abs < 1.0e-2 {
        // 3 significant figures in scientific notation, e.g. 3.45e6
        let mut exponent = abs.log10().floor() as i32;
        let mantissa = v / 10.0f64.powi(exponent);
        // Round mantissa to 2 decimal places (3 sig figs total).
        let mut rounded = (mantissa * 100.0).round() / 100.0;
        // Handle rounding carry (e.g. 9.995 -> 10.0 -> 1.00e(exponent+1)).
        if rounded >= 10.0 {
            rounded /= 10.0;
            exponent += 1;
        }
        format!("{rounded:.2}e{exponent}")
    } else {
        format!("{v}")
    }
}

/// A `DragValue` for editing one end of an axis range.
///
/// The drag speed scales with the axis `span`, so a 10^7-wide axis can be swept
/// in a single drag instead of one unit per pixel, and the value is displayed in
/// compact scientific notation. Typing works too (`1e7`, `5.3e6`, `250000`).
fn range_drag_value(value: &mut f64, span: f64) -> egui::DragValue<'_> {
    egui::DragValue::new(value)
        .speed((span.abs() / 500.0).max(1.0))
        .custom_formatter(|n, _| format_edit_num(n))
        .custom_parser(|s| s.trim().parse::<f64>().ok())
}

/// Zooms an axis range about its centre. `factor < 1` zooms in (narrower
/// range), `factor > 1` zooms out. Log axes are zoomed in log space so the
/// steps feel even across decades.
fn zoom_axis_range(range: &mut (f64, f64), scale: &AxisScale, factor: f64) {
    let (a, b) = *range;
    if !a.is_finite() || !b.is_finite() || factor <= 0.0 {
        return;
    }
    let factor = factor.clamp(0.01, 100.0);
    if !scale.is_linear()
        && let (Some(ta), Some(tb)) = (scale.transform(a), scale.transform(b))
    {
        let center = (ta + tb) / 2.0;
        let half = (tb - ta) / 2.0 * factor;
        *range = (
            scale.inverse(center - half.abs()),
            scale.inverse(center + half.abs()),
        );
        return;
    }
    let center = (a + b) / 2.0;
    let half = (b - a) / 2.0 * factor;
    *range = (center - half.abs(), center + half.abs());
}

/// Moves a single axis bound (minimum or maximum) by a relative step. Used by
/// the − / + buttons that sit beside each axis number so the minimum and the
/// maximum can be adjusted independently of each other.
fn adjust_axis_bound(range: &mut (f64, f64), scale: &AxisScale, is_min: bool, factor: f64) {
    let (a, b) = *range;
    if !a.is_finite() || !b.is_finite() || factor <= 0.0 {
        return;
    }
    let factor = factor.clamp(0.01, 100.0);
    let (next_a, next_b) = if is_min {
        (a * factor, b)
    } else {
        (a, b * factor)
    };
    if next_a > next_b || (scale.positive_only() && next_a <= 0.0) {
        return;
    }
    *range = (next_a, next_b);
}

// -- workspace (de)serialisation helpers -----------------------------------

fn view_mode_id(v: ViewMode) -> &'static str {
    match v {
        ViewMode::Dots => "dots",
        ViewMode::Contour => "contour",
        ViewMode::Histogram => "histogram",
        ViewMode::Pseudocolor => "pseudocolor",
        ViewMode::Zebra => "zebra",
    }
}

fn view_mode_from_id(s: &str) -> ViewMode {
    match s {
        "contour" => ViewMode::Contour,
        "histogram" => ViewMode::Histogram,
        "pseudocolor" => ViewMode::Pseudocolor,
        "zebra" => ViewMode::Zebra,
        _ => ViewMode::Dots,
    }
}

fn label_font_id(f: LabelFont) -> &'static str {
    match f {
        LabelFont::Sans => "sans",
        LabelFont::Serif => "serif",
        LabelFont::Mono => "mono",
    }
}

fn label_font_from_id(s: &str) -> LabelFont {
    match s {
        "serif" => LabelFont::Serif,
        "mono" => LabelFont::Mono,
        _ => LabelFont::Sans,
    }
}

fn quadrant_id(q: &Quadrant) -> &'static str {
    match q {
        Quadrant::UpperLeft => "upper_left",
        Quadrant::UpperRight => "upper_right",
        Quadrant::LowerLeft => "lower_left",
        Quadrant::LowerRight => "lower_right",
    }
}

fn quadrant_from_id(s: &str) -> Quadrant {
    match s {
        "upper_left" => Quadrant::UpperLeft,
        "lower_left" => Quadrant::LowerLeft,
        "lower_right" => Quadrant::LowerRight,
        _ => Quadrant::UpperRight,
    }
}

fn gate_shape_from_kind(kind: &GateKind) -> GateShape {
    match kind {
        GateKind::Rect {
            x_min,
            x_max,
            y_min,
            y_max,
            angle,
        } => GateShape::Rect {
            x_min: *x_min,
            x_max: *x_max,
            y_min: *y_min,
            y_max: *y_max,
            angle: *angle,
        },
        GateKind::Ellipse {
            x_min,
            x_max,
            y_min,
            y_max,
            angle,
        } => GateShape::Ellipse {
            x_min: *x_min,
            x_max: *x_max,
            y_min: *y_min,
            y_max: *y_max,
            angle: *angle,
        },
        GateKind::Polygon { vertices } => GateShape::Polygon {
            vertices: vertices.clone(),
        },
        GateKind::Quadrant { x, y, quad } => GateShape::Quadrant {
            x: *x,
            y: *y,
            quad: quadrant_id(quad).to_string(),
        },
        GateKind::Bilinear {
            x1,
            y1,
            x2,
            y2,
            below,
        } => GateShape::Bilinear {
            x1: *x1,
            y1: *y1,
            x2: *x2,
            y2: *y2,
            below: *below,
        },
        GateKind::Mask { indices } => GateShape::Mask {
            indices: indices.clone(),
        },
    }
}

fn gate_kind_from_shape(shape: &GateShape) -> GateKind {
    match shape {
        GateShape::Rect {
            x_min,
            x_max,
            y_min,
            y_max,
            angle,
        } => GateKind::Rect {
            x_min: *x_min,
            x_max: *x_max,
            y_min: *y_min,
            y_max: *y_max,
            angle: *angle,
        },
        GateShape::Ellipse {
            x_min,
            x_max,
            y_min,
            y_max,
            angle,
        } => GateKind::Ellipse {
            x_min: *x_min,
            x_max: *x_max,
            y_min: *y_min,
            y_max: *y_max,
            angle: *angle,
        },
        GateShape::Polygon { vertices } => GateKind::Polygon {
            vertices: vertices.clone(),
        },
        GateShape::Quadrant { x, y, quad } => GateKind::Quadrant {
            x: *x,
            y: *y,
            quad: quadrant_from_id(quad),
        },
        GateShape::Bilinear {
            x1,
            y1,
            x2,
            y2,
            below,
        } => GateKind::Bilinear {
            x1: *x1,
            y1: *y1,
            x2: *x2,
            y2: *y2,
            below: *below,
        },
        GateShape::Mask { indices } => GateKind::Mask {
            indices: indices.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// Gating-ML import / export (GUI <-> Gating-ML 1.5/2.0)
// ---------------------------------------------------------------------------

/// Build a Gating-ML document from the gates of one sample.
///
/// Gates without a Gating-ML geometry mapping (e.g. bilinear line gates) are
/// skipped and reported through the document's warnings instead of being
/// silently dropped.
fn gatingml_document_from_gates(
    version: crate::gatingml::GatingMlVersion,
    gates: &[GateDef],
    param_names: &[String],
    compensation: Option<CompensationMatrix>,
) -> crate::gatingml::GatingMlDocument {
    let mut doc = crate::gatingml::GatingMlDocument::new(version);
    for gate in gates {
        let shape = gate_shape_from_kind(&gate.kind);
        let x_param = param_names
            .get(gate.x_idx)
            .cloned()
            .unwrap_or_else(|| format!("P{}", gate.x_idx));
        let y_param = param_names
            .get(gate.y_idx)
            .cloned()
            .unwrap_or_else(|| format!("P{}", gate.y_idx));
        let Some(kind) = crate::gatingml::shape_to_gatingml(&shape, x_param, y_param) else {
            doc.warnings.push(format!(
                "gate '{}' has no Gating-ML representation and was skipped",
                gate.name
            ));
            continue;
        };
        doc.gates.push(crate::gatingml::GatingMlGate {
            id: format!("gate-{}", doc.gates.len() + 1),
            parent_id: gate.parent.map(|p| format!("gate-{}", p + 1)),
            name: gate.name.clone(),
            kind,
        });
    }
    if let Some(matrix) = compensation {
        doc.compensation = Some(matrix);
    }
    doc
}

/// Convert a single Gating-ML gate into a GUI gate kind with resolved parameter
/// indices, or `None` when the gate has no geometry mapping or parameter pair.
fn gatingml_gate_to_kind(
    gate: &crate::gatingml::GatingMlGate,
    param_names: &[String],
) -> Option<(GateKind, usize, usize)> {
    let shape = crate::gatingml::gatingml_to_shape(&gate.kind)?;
    let kind = gate_kind_from_shape(&shape);
    let (x_param, y_param) = gatingml_gate_params(&gate.kind)?;
    let x_idx = param_names.iter().position(|n| n == x_param)?;
    let y_idx = param_names.iter().position(|n| n == y_param)?;
    Some((kind, x_idx, y_idx))
}

/// Extract the parameter-name pair of a Gating-ML gate kind, if it has one.
fn gatingml_gate_params(kind: &crate::gatingml::GatingMlGateKind) -> Option<(&str, &str)> {
    match kind {
        crate::gatingml::GatingMlGateKind::Rectangle {
            x_param, y_param, ..
        }
        | crate::gatingml::GatingMlGateKind::Polygon {
            x_param, y_param, ..
        }
        | crate::gatingml::GatingMlGateKind::Ellipse {
            x_param, y_param, ..
        }
        | crate::gatingml::GatingMlGateKind::Quadrant {
            x_param, y_param, ..
        } => Some((x_param, y_param)),
        crate::gatingml::GatingMlGateKind::Boolean { .. } => None,
    }
}

fn label_to_project(l: &LabelStyle) -> ProjectLabel {
    ProjectLabel {
        text: l.text.clone(),
        size: l.size,
        rotation_deg: l.rotation_deg,
        font: label_font_id(l.font).to_string(),
        color: [l.color.r(), l.color.g(), l.color.b()],
    }
}

fn label_from_project(l: &ProjectLabel) -> LabelStyle {
    LabelStyle {
        text: l.text.clone(),
        size: l.size,
        rotation_deg: l.rotation_deg,
        font: label_font_from_id(&l.font),
        color: egui::Color32::from_rgb(l.color[0], l.color[1], l.color[2]),
    }
}

/// Data-space range for auto-scaling that respects the axis scale.
/// Heuristic channel kind from a parameter name prefix (FSC/SSC = scatter,
/// fluorescence-family prefixes = fluorescence, otherwise unknown).
fn parameter_kind(fcs: &FcsFile, idx: usize) -> String {
    let empty = String::new();
    let name = fcs
        .parameters
        .get(idx)
        .map(|p| p.name.to_uppercase())
        .unwrap_or_default();
    let upper = name.to_ascii_uppercase();
    if upper.starts_with("FSC") || upper.starts_with("SSC") {
        "Scatter".to_string()
    } else if upper.starts_with("FL")
        || upper.starts_with("PE")
        || upper.starts_with("FITC")
        || upper.starts_with("PERCP")
        || upper.starts_with("APC")
        || upper.starts_with("BV")
        || upper.starts_with("HE")
        || upper.starts_with("LEX")
    {
        "Fluorescence".to_string()
    } else {
        empty
    }
}

fn bounds_for(values: &[f64], scale: &AxisScale) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for &v in values {
        if !v.is_finite() {
            continue;
        }
        if scale.positive_only() && v <= 0.0 {
            continue;
        }
        min = min.min(v);
        max = max.max(v);
    }
    if !min.is_finite() {
        return match scale {
            AxisScale::Linear => (0.0, 1.0),
            AxisScale::Log => (1.0, 1000.0),
            _ => (-1.0, 1.0),
        };
    }
    if (max - min).abs() < f64::EPSILON {
        let pad = if min.abs() > 1.0 {
            min.abs() * 0.1
        } else {
            1.0
        };
        return (min - pad, max + pad);
    }
    if scale.is_linear() {
        let pad = (max - min) * 0.05;
        return (min - pad, max + pad);
    }
    match (scale.transform(min), scale.transform(max)) {
        (Some(t0), Some(t1)) => {
            let pad = (t1 - t0) * 0.05;
            (scale.inverse(t0 - pad), scale.inverse(t1 + pad))
        }
        _ => {
            let pad = (max - min) * 0.05;
            (min - pad, max + pad)
        }
    }
}

impl FlowersApp {
    fn current_fcs(&self) -> Option<&FcsFile> {
        self.current_sample.map(|i| &self.samples[i].fcs)
    }

    // -- magnetic snapping --------------------------------------------------

    /// Snapping settings for a view whose largest axis span is `span`.
    fn magnetic_settings(&self, span: f64) -> crate::magnetic::SnapSettings {
        crate::magnetic::SnapSettings {
            enabled: self.magnetic_snap,
            radius: (self.magnetic_radius_frac as f64 * span.abs()).max(1e-9),
            strength: self.magnetic_strength as f64,
            max_iterations: 24,
        }
    }

    /// Density field over the current sample's events on one parameter pair.
    ///
    /// Returns `None` when no sample is loaded, the parameters are out of
    /// range, or fewer than two finite events remain. The field covers the
    /// data's bounding box with a small margin.
    fn density_field_for(
        &self,
        x_idx: usize,
        y_idx: usize,
    ) -> Option<crate::magnetic::DensityField> {
        let fcs = self.current_fcs()?;
        let ncols = fcs.data.ncols();
        if x_idx >= ncols || y_idx >= ncols {
            return None;
        }
        let mut points = Vec::with_capacity(fcs.data.nrows());
        let (mut min_x, mut max_x) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
        for row in fcs.data.rows() {
            let x = row[x_idx] as f64;
            let y = row[y_idx] as f64;
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            points.push([x, y]);
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        if points.len() < 2 {
            return None;
        }
        let span_x = (max_x - min_x).max(1e-6);
        let span_y = (max_y - min_y).max(1e-6);
        let pad_x = span_x * 0.02;
        let pad_y = span_y * 0.02;
        Some(crate::magnetic::DensityField::from_points(
            &points,
            min_x - pad_x,
            max_x + pad_x,
            min_y - pad_y,
            max_y + pad_y,
            96,
            96,
        ))
    }

    /// Snap a single drawing point (click, vertex or corner) to nearby density.
    fn snap_drawing_point(
        &self,
        point: [f64; 2],
        x_idx: usize,
        y_idx: usize,
        span: f64,
    ) -> [f64; 2] {
        if !self.magnetic_snap {
            return point;
        }
        let Some(field) = self.density_field_for(x_idx, y_idx) else {
            return point;
        };
        crate::magnetic::snap_to_density(point, &field, &self.magnetic_settings(span))
    }

    /// Snap polygon vertices to nearby density.
    fn snap_vertices(
        &self,
        vertices: &[[f64; 2]],
        x_idx: usize,
        y_idx: usize,
        span: f64,
    ) -> Vec<[f64; 2]> {
        if !self.magnetic_snap {
            return vertices.to_vec();
        }
        let Some(field) = self.density_field_for(x_idx, y_idx) else {
            return vertices.to_vec();
        };
        crate::magnetic::snap_polygon(vertices, &field, &self.magnetic_settings(span))
    }

    /// Snap both ends of a creation drag, keeping the drag valid.
    fn snap_drag(
        &self,
        start: [f64; 2],
        end: [f64; 2],
        x_idx: usize,
        y_idx: usize,
        span: f64,
        new_type: NewGateType,
    ) -> ([f64; 2], [f64; 2]) {
        if !self.magnetic_snap {
            return (start, end);
        }
        let Some(field) = self.density_field_for(x_idx, y_idx) else {
            return (start, end);
        };
        let settings = self.magnetic_settings(span);
        match new_type {
            NewGateType::Rect | NewGateType::Ellipse => {
                let (min_x, max_x, min_y, max_y) =
                    crate::magnetic::snap_rectangle(start, end, &field, &settings);
                // Both corners can land on the same density peak; keep the
                // user's original drag rather than creating a degenerate gate.
                if (max_x - min_x).abs() < f64::EPSILON || (max_y - min_y).abs() < f64::EPSILON {
                    (start, end)
                } else {
                    ([min_x, min_y], [max_x, max_y])
                }
            }
            NewGateType::Bilinear => (
                crate::magnetic::snap_to_density(start, &field, &settings),
                crate::magnetic::snap_to_density(end, &field, &settings),
            ),
            NewGateType::Quadrant | NewGateType::Polygon => (start, end),
        }
    }

    /// Flow_r-inspired colour tokens for the active theme.
    fn palette(&self) -> Palette {
        Palette::for_theme(self.theme)
    }

    // -- compensation -------------------------------------------------------

    fn current_sample_compensated(&self) -> bool {
        self.current_sample
            .and_then(|i| self.samples.get(i))
            .is_some_and(|s| s.compensation.is_some())
    }

    /// Opens the compensation editor, seeding it with the current sample's
    /// matrix (or its acquisition spillover).
    fn open_compensation(&mut self) {
        let draft = self
            .current_sample
            .and_then(|i| self.samples.get(i))
            .and_then(|sample| {
                sample
                    .compensation
                    .clone()
                    .or_else(|| acquisition_compensation(&sample.fcs))
            })
            .or_else(|| {
                self.current_fcs().and_then(|fcs| {
                    CompensationMatrix::identity(default_compensation_channels(fcs)).ok()
                })
            });
        self.comp_draft = draft;
        self.comp_message = if self.comp_draft.is_some() {
            "Edit values or load a matrix CSV.".to_string()
        } else {
            "Load a matrix CSV or select a sample to start.".to_string()
        };
        self.comp_open = true;
    }

    /// Applies the draft matrix to the current sample or every sample.
    fn apply_compensation(&mut self, all: bool) {
        let Some(matrix) = self.comp_draft.clone() else {
            self.comp_message = "No matrix to apply.".to_string();
            return;
        };
        let targets: Vec<usize> = if all {
            (0..self.samples.len()).collect()
        } else {
            self.current_sample.into_iter().collect()
        };
        if targets.is_empty() {
            self.comp_message = "Load a sample first.".to_string();
            return;
        }

        let mut applied = 0usize;
        let mut last_error = String::new();
        for i in targets {
            let (names, descriptions) = match self.samples.get(i) {
                Some(s) => (parameter_names(&s.fcs), parameter_descriptions(&s.fcs)),
                None => continue,
            };
            let sample = &mut self.samples[i];
            match matrix.apply_to(&sample.raw_data, &names, &descriptions) {
                Ok(data) => {
                    sample.fcs.data = data;
                    sample.compensation = Some(matrix.clone());
                    applied += 1;
                }
                Err(e) => last_error = format!("{}: {e}", sample.id),
            }
        }

        if applied > 0 {
            self.dirty = true;
            self.update_plot_points();
            let matched = self
                .current_fcs()
                .map(|fcs| {
                    matrix.matched_count(&parameter_names(fcs), &parameter_descriptions(fcs))
                })
                .unwrap_or(0);
            self.comp_message =
                format!("Applied to {applied} sample(s); {matched} channel(s) matched.");
            self.set_status(format!(
                "Compensation applied ({matched} channels, {applied} sample(s))"
            ));
        } else {
            self.comp_message = if last_error.is_empty() {
                "No samples to compensate.".to_string()
            } else {
                format!("Compensation failed — {last_error}")
            };
        }
    }

    /// Removes compensation from the current sample or every sample, restoring
    /// the original uncompensated data.
    fn clear_compensation(&mut self, all: bool) {
        let targets: Vec<usize> = if all {
            (0..self.samples.len()).collect()
        } else {
            self.current_sample.into_iter().collect()
        };
        let mut cleared = 0usize;
        for i in targets {
            if let Some(sample) = self.samples.get_mut(i)
                && sample.compensation.is_some()
            {
                sample.fcs.data = sample.raw_data.clone();
                sample.compensation = None;
                cleared += 1;
            }
        }
        if cleared > 0 {
            self.dirty = true;
            self.update_plot_points();
            self.comp_message = format!("Removed compensation from {cleared} sample(s).");
            self.set_status("Compensation removed");
        } else {
            self.comp_message = "Those samples are not compensated.".to_string();
        }
    }

    /// Toggles the compensation node visibility in the population tree (C6).
    fn toggle_compensation_node(&mut self) {
        self.comp_node_visible = !self.comp_node_visible;
        self.comp_message = if self.comp_node_visible {
            "Compensation node visible.".to_string()
        } else {
            "Compensation node hidden.".to_string()
        };
    }

    /// Status line for the compensation node (C6).
    fn compensation_node_status(&self) -> String {
        if !self.comp_node_visible {
            return "Compensation node hidden.".to_string();
        }
        let applied = self
            .current_sample
            .and_then(|i| self.samples.get(i))
            .map(|s| s.compensation.is_some())
            .unwrap_or(false);
        if applied {
            "Compensation node active.".to_string()
        } else {
            "Compensation node idle (no matrix applied).".to_string()
        }
    }

    /// Loads the current sample's `$SPILLOVER` into the editor.
    fn load_acquisition_matrix(&mut self) {
        match self.current_fcs().and_then(acquisition_compensation) {
            Some(matrix) => {
                self.comp_draft = Some(matrix);
                self.comp_message =
                    "Loaded the acquisition spillover matrix from $SPILLOVER.".to_string();
            }
            None => {
                self.comp_message = "This sample has no usable $SPILLOVER keyword.".to_string();
            }
        }
    }

    /// Adds every sample channel that is missing from the draft matrix, with
    /// identity coefficients, so a partial matrix can cover the whole file.
    fn comp_add_missing_channels(&mut self) {
        let Some(fcs) = self.current_fcs() else {
            return;
        };
        let names = parameter_names(fcs);
        let Some(matrix) = self.comp_draft.as_mut() else {
            return;
        };
        for name in names {
            if matrix
                .channels
                .iter()
                .any(|c| c.trim().eq_ignore_ascii_case(name.trim()))
            {
                continue;
            }
            let old_n = matrix.channels.len();
            for row in &mut matrix.values {
                row.push(0.0);
            }
            let mut row = vec![0.0f32; old_n + 1];
            row[old_n] = 1.0;
            matrix.values.push(row);
            matrix.channels.push(name);
        }
        self.comp_message = format!(
            "Matrix now covers {} channel(s).",
            self.comp_draft
                .as_ref()
                .map(|m| m.channels.len())
                .unwrap_or(0)
        );
    }

    /// Starts the editor with a fresh identity matrix over the sample channels.
    fn comp_identity(&mut self) {
        let Some(fcs) = self.current_fcs() else {
            self.comp_message = "Load a sample first.".to_string();
            return;
        };
        let channels = default_compensation_channels(fcs);
        match CompensationMatrix::identity(channels) {
            Ok(matrix) => {
                self.comp_draft = Some(matrix);
                self.comp_message = "Started a new identity matrix.".to_string();
            }
            Err(e) => self.comp_message = format!("Could not create matrix: {e}"),
        }
    }

    fn load_file(&mut self, path: &std::path::Path) {
        match std::fs::read(path) {
            Ok(bytes) => match FcsParser::parse_bytes(&bytes) {
                Ok(fcs) => {
                    let id = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let params = fcs.parameters.len();
                    let events = fcs.event_count;
                    let had_gates = !self.gates.is_empty();
                    let (dx, dy) = default_param_pair(&fcs);
                    let (fcs, compensation, raw_data) = with_acquisition_compensation(fcs);
                    let auto_compensated = compensation.is_some();
                    self.samples.push(SampleEntry {
                        id: id.clone(),
                        path: path.to_path_buf(),
                        fcs,
                        raw_data,
                        compensation,
                        apply_gates: !had_gates,
                    });
                    let idx = self.samples.len() - 1;
                    self.current_sample = Some(idx);
                    self.x_param = Some(dx);
                    self.y_param = Some(dy);
                    self.plot_points.clear();
                    // Ask before applying an existing gating strategy to a new
                    // file (the user decides per file).
                    if had_gates {
                        self.pending_apply_prompt = Some(idx);
                    }
                    self.update_plot_points();
                    self.set_status(format!(
                        "Loaded {id}: {events} events × {params} parameters{}",
                        if auto_compensated {
                            " (auto-compensated from $SPILLOVER)"
                        } else {
                            ""
                        }
                    ));
                }
                Err(e) => self.set_status(format!("Error parsing FCS: {e}")),
            },
            Err(e) => self.set_status(format!("Error reading file: {e}")),
        }
    }

    // -- 3D view (V3) -------------------------------------------------------

    /// Open the 3D view and map X/Y/Z to sensible parameters.
    fn open_view3d(&mut self) {
        self.view3d_open = true;
        if let Some(fcs) = self.current_fcs() {
            let n = fcs.parameters.len();
            if n == 0 {
                return;
            }
            let x = self.x_param.unwrap_or(0).min(n - 1);
            let y = self.y_param.unwrap_or(1).min(n - 1);
            self.view3d_spec.x = x;
            self.view3d_spec.y = y;
            if self.view3d_spec.z >= n || self.view3d_spec.z == x || self.view3d_spec.z == y {
                self.view3d_spec.z = (0..n).find(|&i| i != x && i != y).unwrap_or(0);
            }
        }
    }

    /// Render the current 3D view to SVG.
    fn view3d_svg(&self) -> Result<(String, u32, u32), String> {
        let Some(fcs) = self.current_fcs() else {
            return Err("No sample loaded for the 3D view".to_string());
        };
        let (w, h) = (640u32, 480u32);
        let svg = crate::plot3d::render_3d(&fcs.data, &self.view3d_spec, w, h, "#ffffff");
        Ok((svg, w, h))
    }

    /// Export the current 3D view as SVG.
    fn export_view3d_svg(&mut self) {
        match self.view3d_svg() {
            Ok((svg, _, _)) => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("SVG", &["svg"])
                    .set_file_name("view3d.svg")
                    .save_file()
                {
                    match std::fs::write(&path, svg) {
                        Ok(()) => {
                            self.set_status(format!("Exported 3D view to {}", path.display()))
                        }
                        Err(e) => self.set_status(format!("3D export failed: {e}")),
                    }
                }
            }
            Err(e) => self.set_status(e),
        }
    }

    /// Export a rotation animation as a numbered SVG frame sequence.
    fn export_view3d_animation(&mut self) {
        let Some(fcs) = self.current_fcs() else {
            self.set_status("No sample loaded for the 3D view");
            return;
        };
        let Some(dir) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        let (w, h) = (640u32, 480u32);
        let frames = crate::plot3d::rotation_frames_svg(
            &fcs.data,
            &self.view3d_spec,
            self.view3d_frames.max(2),
            10.0,
            w,
            h,
            "#ffffff",
        );
        let mut written = 0usize;
        for (i, svg) in frames.iter().enumerate() {
            let path = dir.join(format!("frame_{i:03}.svg"));
            if std::fs::write(&path, svg).is_err() {
                self.set_status(format!("Failed writing {}", path.display()));
                return;
            }
            written += 1;
        }
        self.set_status(format!(
            "Exported {written} 3D animation frames to {}",
            dir.display()
        ));
    }

    // -- statistics panel (S5) ---------------------------------------------

    /// Open / close the statistics panel window.
    fn open_stats_panel(&mut self) {
        self.stats_panel_open = true;
    }

    /// Rows shown in the statistics panel for the current population.
    fn stats_panel_profiles(&self) -> Vec<crate::statistics::PopulationProfile> {
        let Some(selected) = self.selected_gate else {
            return Vec::new();
        };
        let Some(fcs) = self.current_fcs() else {
            return Vec::new();
        };
        let Some(indices) = self.pop_indices.get(selected).cloned() else {
            return Vec::new();
        };
        let names = parameter_names(fcs);
        let parent_count = self
            .gates
            .get(selected)
            .and_then(|g| g.parent)
            .and_then(|p| self.pop_indices.get(p).map(|v| v.len()));
        let total_count = fcs.event_count;
        vec![crate::statistics::compute_profile(
            "panel",
            &fcs.data,
            &indices,
            &names,
            parent_count,
            Some(total_count),
        )]
    }

    fn stats_panel_rows(&self) -> Vec<crate::statistics::PopulationStat> {
        self.stats_panel_profiles()
            .into_iter()
            .flat_map(|p| p.stats)
            .collect()
    }

    /// Add a copy of a statistic set to the panel.
    fn stats_panel_add_set(&mut self, index: usize) {
        if let Some(set) = self.statistic_sets.get(index) {
            self.statistic_sets.push(set.clone());
        }
    }

    /// Remove the statistic set at `index` from the panel.
    fn stats_panel_remove_set(&mut self, index: usize) {
        if index < self.statistic_sets.len() {
            self.statistic_sets.remove(index);
        }
    }

    /// Copy a statistic set (deep clone) for editing in the panel.
    fn stats_panel_copy_set(&mut self, index: usize) {
        if let Some(set) = self.statistic_sets.get(index).cloned() {
            self.statistic_sets.push(set);
        }
    }

    /// Export the current statistic sets to a JSON file.
    fn stats_panel_export(&mut self) -> Result<(), String> {
        let sets = std::mem::take(&mut self.statistic_sets);
        let json = serde_json::to_string_pretty(&sets).map_err(|e| e.to_string())?;
        let path = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name("statistic_sets.json")
            .save_file();
        let Some(path) = path else {
            self.statistic_sets = sets;
            return Ok(());
        };
        std::fs::write(&path, json).map_err(|e| e.to_string())?;
        self.statistic_sets = sets;
        Ok(())
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
    }

    // -- undo / redo --------------------------------------------------------

    /// Captures the current editable state.
    fn snapshot(&self) -> UndoState {
        UndoState {
            gates: self.gates.clone(),
            selected_gate: self.selected_gate,
            x_param: self.x_param,
            y_param: self.y_param,
            view_mode: self.view_mode,
            x_scale: self.x_scale.clone(),
            y_scale: self.y_scale.clone(),
            point_color: self.point_color,
            gradient: self.gradient,
            density_map: self.density_map,
            contour_levels: self.contour_levels,
            gate_line_width: self.gate_line_width,
            show_median_cross: self.show_median_cross,
            magnetic_snap: self.magnetic_snap,
            magnetic_strength: self.magnetic_strength,
            magnetic_radius_frac: self.magnetic_radius_frac,
            gate_labels: self.gate_labels.clone(),
            x_range: self.x_range,
            y_range: self.y_range,
            layout_panels: self.layout_panels.clone(),
            layout_columns: self.layout_columns,
            layout_selected: self.layout_selected,
        }
    }

    /// Restores a snapshot, refreshing derived data (scatter points, gates).
    fn apply_state(&mut self, state: UndoState) {
        self.gates = state.gates;
        self.selected_gate = state.selected_gate;
        self.x_param = state.x_param;
        self.y_param = state.y_param;
        self.x_scale = state.x_scale;
        self.y_scale = state.y_scale;
        // Recompute scatter points + populations for the restored parameters,
        // then override the auto-fitted axis ranges with the stored ones.
        self.update_plot_points();
        self.x_range = state.x_range;
        self.y_range = state.y_range;
        self.view_mode = state.view_mode;
        self.point_color = state.point_color;
        self.gradient = state.gradient;
        self.density_map = state.density_map;
        self.contour_levels = state.contour_levels;
        self.gate_line_width = state.gate_line_width;
        self.show_median_cross = state.show_median_cross;
        self.magnetic_snap = state.magnetic_snap;
        self.magnetic_strength = state.magnetic_strength;
        self.magnetic_radius_frac = state.magnetic_radius_frac;
        self.gate_labels = state.gate_labels;
        self.layout_panels = state.layout_panels;
        self.layout_columns = state.layout_columns;
        self.layout_selected = state.layout_selected;
        if self
            .layout_selected
            .is_some_and(|s| s >= self.layout_panels.len())
        {
            self.layout_selected = None;
        }
    }

    fn can_undo(&self) -> bool {
        self.history_idx > 0
    }

    fn can_redo(&self) -> bool {
        self.history_idx + 1 < self.history.len()
    }

    fn undo(&mut self) {
        if self.can_undo() {
            self.history_idx -= 1;
            let state = self.history[self.history_idx].clone();
            self.apply_state(state);
            self.history_skip = true;
            self.set_status("Undo");
        }
    }

    fn redo(&mut self) {
        if self.can_redo() {
            self.history_idx += 1;
            let state = self.history[self.history_idx].clone();
            self.apply_state(state);
            self.history_skip = true;
            self.set_status("Redo");
        }
    }

    /// Records the current state if it differs from the last snapshot.
    ///
    /// Called once per frame. While the pointer is down (mid-drag) the state is
    /// not recorded, so an entire drag collapses into a single undo step.
    fn record_history(&mut self, pointer_down: bool) {
        if self.history_skip {
            self.history_skip = false;
            return;
        }
        let current = self.snapshot();
        let changed = self
            .history
            .get(self.history_idx)
            .map(|s| !s.same_document(&current))
            .unwrap_or(true);
        if !changed || pointer_down {
            return;
        }
        // The initial snapshot (and the baseline after New / Load Workspace) is
        // not an edit, so it must not mark a clean workspace as dirty.
        let is_baseline = self.history.is_empty();
        if !is_baseline {
            // The document differs from the last saved state.
            self.dirty = true;
        }
        // Drop any redo entries and append the new state.
        self.history.truncate(self.history_idx + 1);
        self.history.push(current);
        const MAX_HISTORY: usize = 200;
        if self.history.len() > MAX_HISTORY {
            let excess = self.history.len() - MAX_HISTORY;
            self.history.drain(0..excess);
        }
        self.history_idx = self.history.len() - 1;
    }

    // -- workspace (.wsp) ---------------------------------------------------

    /// Resets the application to an empty workspace, keeping the theme.
    fn new_workspace(&mut self) {
        self.dirty = false;
        self.samples.clear();
        self.gates.clear();
        self.layout_panels.clear();
        self.selected_gate = None;
        self.layout_selected = None;
        self.current_sample = None;
        self.x_param = None;
        self.y_param = None;
        self.plot_points.clear();
        self.pop_indices.clear();
        self.children.clear();
        self.pending_apply_prompt = None;
        self.popouts.clear();
        self.active_popout = None;
        self.view_mode = ViewMode::Dots;
        self.x_scale = AxisScale::Linear;
        self.y_scale = AxisScale::Linear;
        self.gradient = true;
        self.density_map = crate::plot::DensityMap::Rainbow;
        self.overlay_populations = false;
        self.transpose = false;
        self.plot_smoothing = 0.0;
        self.plot_resolution = 128;
        self.outlier_clip_percent = 0.0;
        self.large_dots = false;
        self.show_grid = false;
        self.show_medians = false;
        self.show_event_numbers = false;
        self.show_names_frequencies = false;
        self.tick_notation = crate::plot::TickNotation::Auto;
        self.show_axis_labels = true;
        self.show_ticks = true;
        self.show_numbers = true;
        self.show_axis_histograms = false;
        self.contour_levels = 8;
        self.gate_line_width = 1.5;
        self.show_median_cross = false;
        self.magnetic_snap = false;
        self.magnetic_strength = 0.6;
        self.magnetic_radius_frac = 0.04;
        self.gate_labels = GateLabelSettings::default();
        self.autorange_rules = Vec::new();
        self.point_color = egui::Color32::from_rgb(31, 119, 180);
        self.x_range = (0.0, 1.0);
        self.y_range = (0.0, 1.0);
        self.layout_columns = 2;
        self.history.clear();
        self.history_idx = 0;
        self.comp_open = false;
        self.comp_draft = None;
        self.comp_message.clear();
        self.record_history(false);
        self.set_status("New workspace");
    }

    /// Builds a serialisable snapshot of the current analysis.
    fn to_project(&self) -> Project {
        let samples = self
            .samples
            .iter()
            .map(|s| ProjectSample {
                path: s.path.to_string_lossy().to_string(),
                apply_gates: s.apply_gates,
                compensation: s.compensation.clone(),
                transforms: Vec::new(),
            })
            .collect();
        let gates = self
            .gates
            .iter()
            .map(|g| ProjectGate {
                name: g.name.clone(),
                x_idx: g.x_idx,
                y_idx: g.y_idx,
                parent: g.parent,
                owner: g.owner,
                apply_to_all: g.apply_to_all,
                color: [g.color.r(), g.color.g(), g.color.b()],
                kind: gate_shape_from_kind(&g.kind),
            })
            .collect();
        let layout = self
            .layout_panels
            .iter()
            .map(|p| ProjectPanel {
                sample: p.sample,
                x: p.x,
                y: p.y,
                view: view_mode_id(p.view).to_string(),
                gate: p.gate,
                title: p.title.clone(),
                title_size: p.title_size,
                rect: [p.rect.min.x, p.rect.min.y, p.rect.width(), p.rect.height()],
                x_label: label_to_project(&p.x_label),
                y_label: label_to_project(&p.y_label),
            })
            .collect();
        Project {
            version: crate::project::PROJECT_VERSION,
            samples,
            current_sample: self.current_sample,
            x_param: self.x_param,
            y_param: self.y_param,
            selected_gate: self.selected_gate,
            view: ViewSettings {
                mode: view_mode_id(self.view_mode).to_string(),
                x_scale: self.x_scale.key().to_string(),
                y_scale: self.y_scale.key().to_string(),
                point_color: [
                    self.point_color.r(),
                    self.point_color.g(),
                    self.point_color.b(),
                ],
                gradient: self.gradient,
                overlay_populations: self.overlay_populations,
                density_map: self.density_map.id().to_string(),
                transpose: self.transpose,
                plot_smoothing: self.plot_smoothing,
                plot_resolution: self.plot_resolution,
                outlier_clip_percent: self.outlier_clip_percent,
                large_dots: self.large_dots,
                show_grid: self.show_grid,
                show_medians: self.show_medians,
                show_event_numbers: self.show_event_numbers,
                show_names_frequencies: self.show_names_frequencies,
                tick_notation: self.tick_notation.id().to_string(),
                show_axis_labels: self.show_axis_labels,
                show_ticks: self.show_ticks,
                show_numbers: self.show_numbers,
                show_axis_histograms: self.show_axis_histograms,
                contour_levels: self.contour_levels,
                x_range: self.x_range,
                y_range: self.y_range,
                gate_line_width: self.gate_line_width,
                show_median_cross: self.show_median_cross,
                magnetic_snap: self.magnetic_snap,
                magnetic_strength: self.magnetic_strength,
                magnetic_radius_frac: self.magnetic_radius_frac,
                gate_labels: self.gate_labels.clone(),
                autorange_rules: self.autorange_rules.clone(),
            },
            gates,
            layout,
            layout_columns: self.layout_columns,
            statistic_sets: self.statistic_sets.clone(),
            derived: Vec::new(),
        }
    }

    /// Writes the current analysis to `path` as a `.wsp` (JSON) file.
    fn save_workspace(&mut self, path: &std::path::Path) {
        let project = self.to_project();
        match crate::project::to_json(&project) {
            Ok(json) => match std::fs::write(path, json) {
                Ok(()) => {
                    self.dirty = false;
                    self.set_status(format!(
                        "Saved workspace: {} sample(s), {} gate(s)",
                        project.samples.len(),
                        project.gates.len()
                    ));
                }
                Err(e) => self.set_status(format!("Error writing workspace: {e}")),
            },
            Err(e) => self.set_status(format!("Error encoding workspace: {e}")),
        }
    }

    /// Loads a `.wsp` file, re-reading the referenced FCS files.
    fn load_workspace(&mut self, path: &std::path::Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.set_status(format!("Error reading workspace: {e}"));
                return;
            }
        };
        match crate::project::from_json(&text) {
            Ok(project) => self.apply_project(project),
            Err(e) => self.set_status(format!("Error parsing workspace: {e}")),
        }
    }

    /// Replaces the current state with a loaded project.
    fn apply_project(&mut self, project: Project) {
        self.samples.clear();
        self.gates.clear();
        self.layout_panels.clear();
        self.selected_gate = None;
        self.layout_selected = None;
        self.current_sample = None;
        self.pending_apply_prompt = None;
        self.popouts.clear();
        self.active_popout = None;

        // View settings (axis ranges are restored after the samples load, since
        // `update_plot_points` auto-fits them).
        self.view_mode = view_mode_from_id(&project.view.mode);
        self.x_scale = AxisScale::from_key(&project.view.x_scale);
        self.y_scale = AxisScale::from_key(&project.view.y_scale);
        self.statistic_sets = project.statistic_sets.clone();
        let [r, g, b] = project.view.point_color;
        self.point_color = egui::Color32::from_rgb(r, g, b);
        self.gradient = project.view.gradient;
        self.overlay_populations = project.view.overlay_populations;
        self.density_map = crate::plot::DensityMap::from_id(&project.view.density_map)
            .unwrap_or(crate::plot::DensityMap::Rainbow);
        self.transpose = project.view.transpose;
        self.plot_smoothing = project.view.plot_smoothing.clamp(0.0, 1.0);
        self.plot_resolution = project.view.plot_resolution.clamp(16, 512);
        self.outlier_clip_percent = project.view.outlier_clip_percent.clamp(0.0, 50.0);
        self.large_dots = project.view.large_dots;
        self.show_grid = project.view.show_grid;
        self.show_medians = project.view.show_medians;
        self.show_event_numbers = project.view.show_event_numbers;
        self.show_names_frequencies = project.view.show_names_frequencies;
        self.tick_notation = crate::plot::TickNotation::from_id(&project.view.tick_notation)
            .unwrap_or(crate::plot::TickNotation::Auto);
        self.show_axis_labels = project.view.show_axis_labels;
        self.show_ticks = project.view.show_ticks;
        self.show_numbers = project.view.show_numbers;
        self.show_axis_histograms = project.view.show_axis_histograms;
        self.contour_levels = project.view.contour_levels;
        self.gate_line_width = project.view.gate_line_width.max(0.1);
        self.show_median_cross = project.view.show_median_cross;
        self.magnetic_snap = project.view.magnetic_snap;
        self.magnetic_strength = project.view.magnetic_strength.clamp(0.0, 1.0);
        self.magnetic_radius_frac = project.view.magnetic_radius_frac.clamp(0.001, 0.5);
        self.gate_labels = project.view.gate_labels.clone();
        self.autorange_rules = project.view.autorange_rules.clone();

        // Load the referenced FCS files.
        let mut missing: Vec<String> = Vec::new();
        for s in &project.samples {
            let p = std::path::Path::new(&s.path);
            match std::fs::read(p) {
                Ok(bytes) => match FcsParser::parse_bytes(&bytes) {
                    Ok(fcs) => {
                        let id = p
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let (fcs, compensation, raw_data) =
                            with_compensation(fcs, s.compensation.as_ref());
                        self.samples.push(SampleEntry {
                            id,
                            path: p.to_path_buf(),
                            fcs,
                            raw_data,
                            compensation,
                            apply_gates: s.apply_gates,
                        });
                    }
                    Err(e) => missing.push(format!("{} (parse: {e})", s.path)),
                },
                Err(e) => missing.push(format!("{} ({e})", s.path)),
            }
        }

        // Gates.
        for pg in &project.gates {
            self.gates.push(GateDef {
                name: pg.name.clone(),
                x_idx: pg.x_idx,
                y_idx: pg.y_idx,
                parent: pg.parent,
                kind: gate_kind_from_shape(&pg.kind),
                color: egui::Color32::from_rgb(pg.color[0], pg.color[1], pg.color[2]),
                owner: pg.owner,
                apply_to_all: pg.apply_to_all,
            });
        }

        // Layout panels.
        self.layout_columns = project.layout_columns.max(1);
        for pp in &project.layout {
            if pp.sample >= self.samples.len() {
                continue;
            }
            let preview = self.make_layout_preview(pp.sample, pp.x, pp.y);
            self.layout_panels.push(LayoutPanelSpec {
                sample: pp.sample,
                x: pp.x,
                y: pp.y,
                view: view_mode_from_id(&pp.view),
                gate: pp.gate,
                title: pp.title.clone(),
                title_size: pp.title_size,
                rect: egui::Rect::from_min_size(
                    egui::pos2(pp.rect[0], pp.rect[1]),
                    egui::vec2(pp.rect[2].max(1.0), pp.rect[3].max(1.0)),
                ),
                preview,
                x_label: label_from_project(&pp.x_label),
                y_label: label_from_project(&pp.y_label),
            });
        }

        // Selection.
        self.current_sample = project
            .current_sample
            .filter(|&i| i < self.samples.len())
            .or_else(|| (!self.samples.is_empty()).then_some(0));
        self.x_param = project.x_param;
        self.y_param = project.y_param;
        self.selected_gate = project.selected_gate.filter(|&i| i < self.gates.len());
        if let Some(fcs) = self.current_fcs() {
            let (dx, dy) = default_param_pair(fcs);
            if self.x_param.is_none() {
                self.x_param = Some(dx);
            }
            if self.y_param.is_none() {
                self.y_param = Some(dy);
            }
        }
        self.update_plot_points();
        // Override the auto-fitted ranges with the saved ones.
        self.x_range = project.view.x_range;
        self.y_range = project.view.y_range;

        // The loaded state is the new baseline for undo / redo.
        self.history.clear();
        self.history_idx = 0;
        self.record_history(false);
        self.dirty = false;

        if missing.is_empty() {
            self.set_status(format!(
                "Loaded workspace: {} sample(s), {} gate(s)",
                self.samples.len(),
                self.gates.len()
            ));
        } else {
            self.set_status(format!(
                "Loaded workspace; {} file(s) could not be opened: {}",
                missing.len(),
                missing.join("; ")
            ));
        }
    }

    /// Load files (e.g. from the command line) and prime the first plot.
    pub fn open_paths(&mut self, paths: &[std::path::PathBuf]) {
        for p in paths {
            self.load_file(p);
        }
    }

    /// Rebuild scatter points, data ranges and axis ranges for the current
    /// sample/parameter selection.
    fn update_plot_points(&mut self) {
        self.plot_points.clear();
        let Some(fcs) = self.current_fcs() else {
            self.recompute_populations();
            return;
        };
        let (Some(x), Some(y)) = (self.x_param, self.y_param) else {
            self.recompute_populations();
            return;
        };
        let (_, n_params) = fcs.data.dim();
        if x >= n_params || y >= n_params {
            return;
        }

        let xs: Vec<f64> = fcs.data.column(x).iter().map(|v| *v as f64).collect();
        let ys: Vec<f64> = fcs.data.column(y).iter().map(|v| *v as f64).collect();
        self.plot_points = xs.iter().zip(ys.iter()).map(|(&a, &b)| [a, b]).collect();

        self.x_data_range = bounds_for(&xs, &self.x_scale);
        self.y_data_range = bounds_for(&ys, &self.y_scale);
        self.x_range = self.x_data_range;
        self.y_range = self.y_data_range;
        self.recompute_populations();
    }

    /// Recompute the event indices of every gate for the current sample.
    fn recompute_populations(&mut self) {
        let apply_gates = self
            .current_sample
            .and_then(|i| self.samples.get(i))
            .map(|s| s.apply_gates)
            .unwrap_or(false);
        self.pop_indices = match self.current_fcs() {
            Some(fcs) => {
                compute_gate_populations(&fcs.data, &self.gates, self.current_sample, apply_gates)
            }
            None => vec![Vec::new(); self.gates.len()],
        };
        self.children = children_map(&self.gates);
    }

    // -- custom lookup transform (T3) --------------------------------------

    /// Open the native file dialog and load a lookup table (CSV) into the app.
    ///
    /// Returns the number of calibration points loaded, or a message when the
    /// file was rejected.
    fn open_lookup_loader(&mut self) -> Result<usize, String> {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("CSV", &["csv"])
            .pick_file()
        {
            return self.load_lookup(&path);
        }
        Ok(0)
    }

    /// Load and validate a lookup table from a CSV file, then attach it to the
    /// currently selected axis scale (X, Y, or both, depending on the caller).
    fn load_lookup(&mut self, path: &std::path::Path) -> Result<usize, String> {
        let source = path.display().to_string();
        let table = LookupTable::load(path).map_err(|e| format!("{e}"))?;
        let point_count = table.x().len();
        self.lookup_table = table;
        self.lookup_source = Some(source.clone());
        self.lookup_message = String::new();
        Ok(point_count)
    }

    /// Persist the active lookup table to a CSV file.
    fn save_lookup(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.lookup_table.save(path).map_err(|e| format!("{e}"))
    }

    /// Apply the active lookup table to the X and Y axes and refit ranges.
    ///
    /// Returns false when the table has fewer than two calibration points.
    fn apply_lookup_to_both(&mut self) -> bool {
        if self.lookup_table.x().len() < 2 {
            self.lookup_message = "Lookup table needs at least two calibration points".to_string();
            return false;
        }
        let scale = self.lookup_table.to_scale();
        self.x_scale = scale.clone();
        self.y_scale = scale;
        self.auto_x();
        self.auto_y();
        self.lookup_message = String::new();
        true
    }

    /// Forget the custom lookup table and restore the linear scale.
    fn clear_lookup(&mut self) {
        self.lookup_table = LookupTable::default();
        self.lookup_source = None;
        self.lookup_message = String::new();
        self.x_scale = AxisScale::Linear;
        self.y_scale = AxisScale::Linear;
        self.auto_x();
        self.auto_y();
    }

    /// Auto-adjust an axis to fit the current points, consulting automatic
    /// autorange rules (T5) before falling back to the data-driven bounds.
    fn auto_x(&mut self) {
        let values: Vec<f64> = self.plot_points.iter().map(|p| p[0]).collect();
        let bounds = self
            .autorange_bounds(&values, self.x_param.unwrap_or(0))
            .unwrap_or_else(|| bounds_for(&values, &self.x_scale));
        self.x_data_range = bounds;
        self.x_range = self.x_data_range;
    }

    fn auto_y(&mut self) {
        let values: Vec<f64> = self.plot_points.iter().map(|p| p[1]).collect();
        let bounds = self
            .autorange_bounds(&values, self.y_param.unwrap_or(0))
            .unwrap_or_else(|| bounds_for(&values, &self.y_scale));
        self.y_data_range = bounds;
        self.y_range = self.y_data_range;
    }

    // -- automatic autorange rules (T5) --------------------------------------

    /// Resolve an automatic (min, max) axis window from the enabled autorange
    /// rules for the parameter at `param_idx`, or `None` when no rule produces
    /// a window.
    fn autorange_bounds(&self, values: &[f64], param_idx: usize) -> Option<(f64, f64)> {
        let fcs = self.current_fcs()?;
        let names = parameter_names(fcs);
        let name = names.get(param_idx)?;
        let kind = parameter_kind(fcs, param_idx);
        let keywords = fcs.keywords.clone();
        let mut candidates: Vec<&AutorangeRule> =
            self.autorange_rules.iter().filter(|r| r.enabled).collect();
        candidates.sort_by_key(|a| std::cmp::Reverse(a.priority));
        for rule in candidates {
            if !rule.applies(name, &kind, &keywords) {
                continue;
            }
            if let Some(bounds) = rule.resolve(values) {
                return Some(bounds);
            }
        }
        None
    }

    /// Add a default automatic autorange rule (FSC, linear bounds).
    fn add_autorange_rule(&mut self) {
        let n = self.autorange_rules.len();
        self.autorange_rules.push(AutorangeRule {
            id: format!("autorange-{}", n),
            name: format!("FSC autorange {}", n),
            matcher: crate::transform::AutorangeMatcher::Name("FSC*".to_string()),
            limit: crate::transform::AutorangeLimit::MinMax { min: 0.0, max: 1.0 },
            ..Default::default()
        });
    }

    /// Remove an automatic autorange rule by index.
    fn remove_autorange_rule(&mut self, index: usize) {
        if index < self.autorange_rules.len() {
            self.autorange_rules.remove(index);
        }
    }

    // -- gate creation ------------------------------------------------------

    /// Add a single gate under an explicit parent, returning its index.
    fn push_gate(
        &mut self,
        name: String,
        kind: GateKind,
        parent: Option<usize>,
        x_idx: usize,
        y_idx: usize,
    ) -> usize {
        let color = gate_color(self.gates.len());
        self.gates.push(GateDef {
            name,
            x_idx,
            y_idx,
            parent,
            kind,
            color,
            owner: self.current_sample,
            apply_to_all: true,
        });
        self.gates.len() - 1
    }

    // -- gate clipboard / duplication / synchronization --------------------

    /// Serialize a gate for the internal clipboard, returning the JSON text.
    ///
    /// The caller may put the returned text on the system clipboard as well;
    /// [`Self::paste_gate`] reads the app's internal copy.
    fn copy_gate(&mut self, idx: usize) -> Option<String> {
        let gate = self.gates.get(idx)?;
        let copied = CopiedGate {
            name: gate.name.clone(),
            x_idx: gate.x_idx,
            y_idx: gate.y_idx,
            parent: gate.parent,
            color: [gate.color.r(), gate.color.g(), gate.color.b()],
            apply_to_all: gate.apply_to_all,
            kind: gate_shape_from_kind(&gate.kind),
        };
        let json = serde_json::to_string(&copied).ok()?;
        self.gate_clipboard = Some(json.clone());
        self.set_status(format!("Copied gate '{}'", gate.name));
        Some(json)
    }

    /// Paste the clipboard gate as a sibling of its original parent.
    ///
    /// The pasted gate keeps the copied name, so it can later be synchronised
    /// with [`Self::sync_named_gates`]. When the original parent no longer
    /// exists, the currently selected gate (or the root) is used instead.
    /// Returns the new gate's index.
    fn paste_gate(&mut self) -> Option<usize> {
        let json = self.gate_clipboard.clone()?;
        let copied: CopiedGate = serde_json::from_str(&json).ok()?;
        let parent = match copied.parent {
            Some(p) if p < self.gates.len() => Some(p),
            _ => self.selected_gate.filter(|&p| p < self.gates.len()),
        };
        let kind = gate_kind_from_shape(&copied.kind);
        self.gates.push(GateDef {
            name: copied.name.clone(),
            x_idx: copied.x_idx,
            y_idx: copied.y_idx,
            parent,
            kind,
            color: egui::Color32::from_rgb(copied.color[0], copied.color[1], copied.color[2]),
            owner: self.current_sample,
            apply_to_all: copied.apply_to_all,
        });
        let index = self.gates.len() - 1;
        self.selected_gate = Some(index);
        self.sync_name_edit();
        self.recompute_populations();
        self.set_status(format!("Pasted gate '{}'", copied.name));
        Some(index)
    }

    /// Duplicate a gate as its sibling, preserving parent, owner and flags.
    ///
    /// The copy gets a unique `<name> copy` name. Returns the new index.
    fn duplicate_gate(&mut self, idx: usize) -> Option<usize> {
        let source = self.gates.get(idx)?.clone();
        let mut copy = source;
        copy.name = unique_gate_name(&self.gates, &format!("{} copy", copy.name));
        self.gates.push(copy);
        let index = self.gates.len() - 1;
        self.selected_gate = Some(index);
        self.sync_name_edit();
        self.recompute_populations();
        self.set_status(format!("Duplicated gate '{}'", self.gates[index].name));
        Some(index)
    }

    /// Copy one gate's geometry onto every same-named gate on the same pair.
    ///
    /// This is the explicit synchronisation step for cloned populations: gates
    /// that share a name and parameter pair are made identical. Returns the
    /// number of gates that changed.
    fn sync_named_gates(&mut self, source: usize) -> usize {
        let Some(gate) = self.gates.get(source) else {
            return 0;
        };
        let name = gate.name.clone();
        let x_idx = gate.x_idx;
        let y_idx = gate.y_idx;
        let kind = gate.kind.clone();
        let mut changed = 0;
        for (i, other) in self.gates.iter_mut().enumerate() {
            if i == source || other.name != name || other.x_idx != x_idx || other.y_idx != y_idx {
                continue;
            }
            if other.kind != kind {
                other.kind = kind.clone();
                changed += 1;
            }
        }
        if changed > 0 {
            self.recompute_populations();
        }
        self.set_status(format!(
            "Synchronised {changed} gate(s) named '{name}' ({x_idx}/{y_idx})"
        ));
        changed
    }

    /// Create gate(s) from a drag rectangle for an explicit parameter pair.
    /// Returns the indices of the gates that were created.
    #[allow(clippy::too_many_arguments)]
    fn create_gates(
        &mut self,
        start: [f64; 2],
        end: [f64; 2],
        x_idx: usize,
        y_idx: usize,
        parent: Option<usize>,
        new_type: NewGateType,
        span: f64,
    ) -> Vec<usize> {
        let mut created = Vec::new();
        let (start, end) = self.snap_drag(start, end, x_idx, y_idx, span, new_type);
        let x_min = start[0].min(end[0]);
        let x_max = start[0].max(end[0]);
        let y_min = start[1].min(end[1]);
        let y_max = start[1].max(end[1]);
        let cx = (x_min + x_max) / 2.0;
        let cy = (y_min + y_max) / 2.0;

        match new_type {
            NewGateType::Rect => {
                if (x_max - x_min).abs() < f64::EPSILON || (y_max - y_min).abs() < f64::EPSILON {
                    return created;
                }
                let n = self.gates.len() + 1;
                created.push(self.push_gate(
                    format!("Rect {n}"),
                    GateKind::Rect {
                        x_min,
                        x_max,
                        y_min,
                        y_max,
                        angle: 0.0,
                    },
                    parent,
                    x_idx,
                    y_idx,
                ));
            }
            NewGateType::Ellipse => {
                if (x_max - x_min).abs() < f64::EPSILON || (y_max - y_min).abs() < f64::EPSILON {
                    return created;
                }
                let n = self.gates.len() + 1;
                created.push(self.push_gate(
                    format!("Ellipse {n}"),
                    GateKind::Ellipse {
                        x_min,
                        x_max,
                        y_min,
                        y_max,
                        angle: 0.0,
                    },
                    parent,
                    x_idx,
                    y_idx,
                ));
            }
            NewGateType::Polygon => {
                if (x_max - x_min).abs() < f64::EPSILON || (y_max - y_min).abs() < f64::EPSILON {
                    return created;
                }
                let n = self.gates.len() + 1;
                let verts =
                    regular_polygon(cx, cy, (x_max - x_min) / 2.0, (y_max - y_min) / 2.0, 5);
                let verts = self.snap_vertices(&verts, x_idx, y_idx, span);
                created.push(self.push_gate(
                    format!("Polygon {n}"),
                    GateKind::Polygon { vertices: verts },
                    parent,
                    x_idx,
                    y_idx,
                ));
            }
            NewGateType::Quadrant => {
                let split = self.snap_drawing_point([cx, cy], x_idx, y_idx, span);
                created = self.create_quadrant_at(split, x_idx, y_idx, parent);
            }
            NewGateType::Bilinear => {
                // Use the drag itself as the dividing line: the gate is the
                // region under that line, bounded by the segment's X extent.
                let len = ((end[0] - start[0]).powi(2) + (end[1] - start[1]).powi(2)).sqrt();
                if len < f64::EPSILON {
                    return created;
                }
                let n = self.gates.len() + 1;
                created.push(self.push_gate(
                    format!("Bilinear {n}"),
                    GateKind::Bilinear {
                        x1: start[0],
                        y1: start[1],
                        x2: end[0],
                        y2: end[1],
                        below: true,
                    },
                    parent,
                    x_idx,
                    y_idx,
                ));
            }
        }
        self.finish_gate_creation();
        created
    }

    /// Create gate(s) from a drag on the main plot (uses the current X/Y pair).
    fn create_gate_from_drag(&mut self, start: [f64; 2], end: [f64; 2]) {
        let (Some(x), Some(y)) = (self.x_param, self.y_param) else {
            return;
        };
        let parent = self.selected_gate;
        let ty = self.new_gate_type;
        let span = (self.x_range.1 - self.x_range.0)
            .abs()
            .max((self.y_range.1 - self.y_range.0).abs())
            .max(1e-9);
        let created = self.create_gates(start, end, x, y, parent, ty, span);
        if let Some(&last) = created.last() {
            self.selected_gate = Some(last);
        }
    }

    /// Create the four quadrant gates at a split point (as siblings).
    fn create_quadrant_at(
        &mut self,
        split: [f64; 2],
        x_idx: usize,
        y_idx: usize,
        parent: Option<usize>,
    ) -> Vec<usize> {
        let base = self.gates.len() + 1;
        let mut created = Vec::new();
        for (i, quad) in [
            Quadrant::UpperLeft,
            Quadrant::UpperRight,
            Quadrant::LowerLeft,
            Quadrant::LowerRight,
        ]
        .into_iter()
        .enumerate()
        {
            let kind = GateKind::Quadrant {
                x: split[0],
                y: split[1],
                quad: quad.clone(),
            };
            created.push(self.push_gate(
                format!("Q{base}.{} {}", i + 1, quad_label(&quad)),
                kind,
                parent,
                x_idx,
                y_idx,
            ));
        }
        created
    }

    /// Create a single line gate as a horizontal threshold at `center[1]`,
    /// spanning `center[0] ± radius`. Used by "+ Add Gate" and click-to-place.
    fn create_bilinear_at(
        &mut self,
        center: [f64; 2],
        radius: f64,
        x_idx: usize,
        y_idx: usize,
        parent: Option<usize>,
    ) -> Vec<usize> {
        let base = self.gates.len() + 1;
        let r = radius.max(1.0);
        let kind = GateKind::Bilinear {
            x1: center[0] - r,
            y1: center[1],
            x2: center[0] + r,
            y2: center[1],
            below: true,
        };
        vec![self.push_gate(format!("Bilinear {base}"), kind, parent, x_idx, y_idx)]
    }

    fn finish_gate_creation(&mut self) {
        self.interact.drag_start = None;
        self.interact.drag_current = None;
        self.sync_name_edit();
        self.recompute_populations();
        self.set_status(format!("{} gates defined", self.gates.len()));
    }

    fn sync_name_edit(&mut self) {
        self.name_edit = self
            .selected_gate
            .and_then(|i| self.gates.get(i))
            .map(|g| g.name.clone())
            .unwrap_or_default();
    }

    /// Remove a gate, re-parenting its children and shifting indices.
    fn remove_gate(&mut self, idx: usize) {
        if idx >= self.gates.len() {
            return;
        }
        let removed_parent = self.gates[idx].parent;
        self.gates.remove(idx);
        for gate in &mut self.gates {
            match gate.parent {
                Some(p) if p == idx => gate.parent = removed_parent,
                Some(p) if p > idx => gate.parent = Some(p - 1),
                _ => {}
            }
        }
        self.selected_gate = if self.gates.is_empty() {
            None
        } else {
            Some(idx.min(self.gates.len() - 1))
        };
        for st in &mut self.popouts {
            st.gate = match st.gate {
                Some(g) if g == idx => None,
                Some(g) if g > idx => Some(g - 1),
                other => other,
            };
            st.selected_gate = None;
        }
        self.sync_name_edit();
        self.recompute_populations();
        self.set_status("Removed gate");
    }

    /// Commits a gate-geometry update from a manipulation (drag, nudge, scale,
    /// numeric edit) onto the gate list, keeping quadrant groups consistent.
    ///
    /// The four quadrants of a split are four sibling gates that share one
    /// split point. Applying a move / resize / split edit to one wedge means
    /// applying it to the shared split, so the sibling wedges are shifted by
    /// the same delta. Without this, moving a quadrant gate would only move the
    /// dragged wedge and the remaining three would keep their stale splits.
    fn commit_gate_edit(&mut self, gate: usize, old: &GateKind, new: &GateKind) {
        if let (Some(old_split), Some(new_split)) = (old.quadrant_split(), new.quadrant_split()) {
            let ox = old_split[0];
            let oy = old_split[1];
            let x = new_split[0];
            let y = new_split[1];
            let dx = x - ox;
            let dy = y - oy;
            if dx == 0.0 && dy == 0.0 {
                return;
            }
            let (x_idx, y_idx) = match self.gates.get(gate) {
                Some(g) => (g.x_idx, g.y_idx),
                None => return,
            };
            for (i, g) in self.gates.iter_mut().enumerate() {
                if i == gate {
                    continue;
                }
                if let Some(sx_yn) = g.kind.quadrant_split() {
                    let (sx, sy) = (sx_yn[0], sx_yn[1]);
                    // Sibling wedge of the same split (same parameter pair) moves
                    // along with the rest of the group.
                    let is_sibling = g.x_idx == x_idx
                        && g.y_idx == y_idx
                        && (sx - ox).abs() < 1e-9
                        && (sy - oy).abs() < 1e-9;
                    if is_sibling {
                        g.kind.translate(dx, dy);
                    }
                }
            }
        }
    }

    /// Nudge a gate by a data-unit delta, keeping quadrant groups in sync.
    fn nudge_gate(&mut self, gate: usize, dx: f64, dy: f64) {
        let old = self.gates.get(gate).map(|g| g.kind.clone());
        if let Some(g) = self.gates.get_mut(gate) {
            g.kind.translate(dx, dy);
        }
        if let Some(old) = old {
            let new_kind = self.gates.get(gate).map(|g| g.kind.clone());
            if let Some(new_kind) = new_kind {
                self.commit_gate_edit(gate, &old, &new_kind);
            }
        }
    }

    /// Rotate a gate about a point by a delta, keeping quadrant groups in sync.
    fn rotate_gate(&mut self, gate: usize, center: [f64; 2], delta: f64) {
        let old = self.gates.get(gate).map(|g| g.kind.clone());
        if let Some(g) = self.gates.get_mut(gate) {
            g.kind.rotate_about(center, delta);
        }
        if let Some(old) = old {
            let new_kind = self.gates.get(gate).map(|g| g.kind.clone());
            if let Some(new_kind) = new_kind {
                self.commit_gate_edit(gate, &old, &new_kind);
            }
        }
    }

    /// Scale a gate about a point by a factor, keeping quadrant groups in sync.
    fn scale_gate(&mut self, gate: usize, center: [f64; 2], factor: f64) {
        let old = self.gates.get(gate).map(|g| g.kind.clone());
        if let Some(g) = self.gates.get_mut(gate) {
            g.kind.scale_about(center, factor);
        }
        if let Some(old) = old {
            let new_kind = self.gates.get(gate).map(|g| g.kind.clone());
            if let Some(new_kind) = new_kind {
                self.commit_gate_edit(gate, &old, &new_kind);
            }
        }
    }

    /// Convert a rectangle, ellipse or polygon gate to another of those shapes.
    ///
    /// Conversion goes through the parameter-independent
    /// [`crate::gating::GateGeometry`] type: rectangles and ellipses use each
    /// other's bounding geometry, and polygons are sampled with 64 vertices.
    /// Quadrant and bilinear gates cannot be converted.
    fn convert_gate(&mut self, gate: usize, target: crate::gating::GateShape) {
        let Some(def) = self.gates.get(gate) else {
            return;
        };
        let geometry = match &def.kind {
            GateKind::Rect {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
            } => crate::gating::GateGeometry::Rectangle {
                min_x: *x_min as f32,
                max_x: *x_max as f32,
                min_y: *y_min as f32,
                max_y: *y_max as f32,
                angle: *angle as f32,
            },
            GateKind::Ellipse {
                x_min,
                x_max,
                y_min,
                y_max,
                angle,
            } => crate::gating::GateGeometry::Ellipse {
                center_x: ((x_min + x_max) / 2.0) as f32,
                center_y: ((y_min + y_max) / 2.0) as f32,
                radius_x: ((x_max - x_min).abs() / 2.0) as f32,
                radius_y: ((y_max - y_min).abs() / 2.0) as f32,
                angle: *angle as f32,
            },
            GateKind::Polygon { vertices } => crate::gating::GateGeometry::Polygon {
                vertices: vertices
                    .iter()
                    .map(|v| (v[0] as f32, v[1] as f32))
                    .collect(),
            },
            GateKind::Quadrant { .. } | GateKind::Bilinear { .. } => {
                self.set_status("Only rectangle, ellipse and polygon gates can be converted");
                return;
            }
            GateKind::Mask { .. } => {
                self.set_status("Mask gates cannot be converted");
                return;
            }
        };
        let new_kind = match geometry.convert(target) {
            crate::gating::GateGeometry::Rectangle {
                min_x,
                max_x,
                min_y,
                max_y,
                angle,
            } => GateKind::Rect {
                x_min: min_x as f64,
                x_max: max_x as f64,
                y_min: min_y as f64,
                y_max: max_y as f64,
                angle: angle as f64,
            },
            crate::gating::GateGeometry::Ellipse {
                center_x,
                center_y,
                radius_x,
                radius_y,
                angle,
            } => GateKind::Ellipse {
                x_min: (center_x - radius_x) as f64,
                x_max: (center_x + radius_x) as f64,
                y_min: (center_y - radius_y) as f64,
                y_max: (center_y + radius_y) as f64,
                angle: angle as f64,
            },
            crate::gating::GateGeometry::Polygon { vertices } => GateKind::Polygon {
                vertices: vertices
                    .iter()
                    .map(|&(x, y)| [x as f64, y as f64])
                    .collect(),
            },
        };
        let old = self.gates[gate].kind.clone();
        self.gates[gate].kind = new_kind.clone();
        self.commit_gate_edit(gate, &old, &new_kind);
        self.recompute_populations();
        self.set_status(format!(
            "Converted '{}' to {}",
            self.gates[gate].name,
            target.label()
        ));
    }

    fn add_default_gate(&mut self) {
        let (Some(x_idx), Some(y_idx)) = (self.x_param, self.y_param) else {
            self.set_status("Select X and Y parameters first");
            return;
        };
        let parent = self.selected_gate;
        let cx = (self.x_range.0 + self.x_range.1) / 2.0;
        let cy = (self.y_range.0 + self.y_range.1) / 2.0;
        let base = self.gates.len() + 1;
        let created = match self.new_gate_type {
            NewGateType::Quadrant => self.create_quadrant_at([cx, cy], x_idx, y_idx, parent),
            NewGateType::Bilinear => {
                let r = ((self.x_range.1 - self.x_range.0)
                    .abs()
                    .max((self.y_range.1 - self.y_range.0).abs()))
                    / 2.0;
                self.create_bilinear_at([cx, cy], r, x_idx, y_idx, parent)
            }
            NewGateType::Rect => vec![self.push_gate(
                format!("Rect {base}"),
                GateKind::Rect {
                    x_min: self.x_range.0,
                    x_max: self.x_range.1,
                    y_min: self.y_range.0,
                    y_max: self.y_range.1,
                    angle: 0.0,
                },
                parent,
                x_idx,
                y_idx,
            )],
            NewGateType::Ellipse => vec![self.push_gate(
                format!("Ellipse {base}"),
                GateKind::Ellipse {
                    x_min: self.x_range.0,
                    x_max: self.x_range.1,
                    y_min: self.y_range.0,
                    y_max: self.y_range.1,
                    angle: 0.0,
                },
                parent,
                x_idx,
                y_idx,
            )],
            NewGateType::Polygon => {
                let rx = (self.x_range.1 - self.x_range.0).abs() / 2.0;
                let ry = (self.y_range.1 - self.y_range.0).abs() / 2.0;
                let verts = regular_polygon(cx, cy, rx, ry, 5);
                vec![self.push_gate(
                    format!("Polygon {base}"),
                    GateKind::Polygon { vertices: verts },
                    parent,
                    x_idx,
                    y_idx,
                )]
            }
        };
        if let Some(&last) = created.last() {
            self.selected_gate = Some(last);
        }
        self.finish_gate_creation();
    }

    /// Import a mask file and add it as a mask gate in the current sample.
    fn import_mask(&mut self, path: &std::path::Path) -> Result<(), String> {
        let bytes = std::fs::read(path)
            .map_err(|e| format!("failed to read mask file {}: {e}", path.display()))?;
        let file_mask = if std::str::from_utf8(&bytes).is_ok() {
            FileMaskGate::from_text(path, std::str::from_utf8(&bytes).unwrap())
                .map_err(|e| format!("invalid mask file {}: {e}", path.display()))?
        } else {
            FileMaskGate::load(path, MaskFileFormat::Bits)
                .map_err(|e| format!("failed to read mask file {}: {e}", path.display()))?
        };
        let indices: Vec<usize> = file_mask.indices().to_vec();
        let name = unique_gate_name(&self.gates, "Mask");
        let index = self.push_gate(
            name.clone(),
            GateKind::Mask {
                indices: indices.clone(),
            },
            None,
            0,
            0,
        );
        self.selected_gate = Some(index);
        self.recompute_populations();
        self.set_status(format!(
            "Imported mask from {}: {} event(s) selected",
            path.display(),
            indices.len()
        ));
        Ok(())
    }

    /// Export the current sample's gates as a Gating-ML document and write it
    /// to `path`.
    fn export_gatingml(&mut self, path: &std::path::Path) -> Result<(), String> {
        let version = crate::gatingml::GatingMlVersion::V2_0;
        let Some(fcs) = self.current_fcs().cloned() else {
            return Err("Load an FCS file before exporting Gating-ML.".to_string());
        };
        let param_names: Vec<String> = fcs.parameters.iter().map(|p| p.name.clone()).collect();
        let compensation = self
            .current_sample
            .and_then(|i| self.samples.get(i))
            .and_then(|s| s.compensation.clone());
        let gates: Vec<GateDef> = self
            .gates
            .iter()
            .filter(|g| g.owner == self.current_sample || g.owner.is_none())
            .cloned()
            .collect();
        let doc = gatingml_document_from_gates(version, &gates, &param_names, compensation);
        doc.save(path)
            .map_err(|e| format!("failed to write Gating-ML file {}: {e}", path.display()))?;
        Ok(())
    }

    /// Import gates from a Gating-ML document into the current sample's gate
    /// list, preserving the gate hierarchy.
    fn import_gatingml(&mut self, path: &std::path::Path) -> Result<(), String> {
        let doc = crate::gatingml::GatingMlDocument::load(path)
            .map_err(|e| format!("failed to read Gating-ML file {}: {e}", path.display()))?;
        let fcs = self
            .current_fcs()
            .cloned()
            .ok_or_else(|| "Load an FCS file before importing Gating-ML.".to_string())?;
        let param_names: Vec<String> = fcs.parameters.iter().map(|p| p.name.clone()).collect();
        let mut warnings = doc.warnings.clone();
        let mut added = 0usize;
        let mut id_to_index: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for gate in &doc.gates {
            let Some((kind, x_idx, y_idx)) = gatingml_gate_to_kind(gate, &param_names) else {
                warnings.push(format!(
                    "gate '{}' was skipped (no supported geometry or parameter pair)",
                    gate.name
                ));
                continue;
            };
            let name = unique_gate_name(&self.gates, &gate.name);
            let parent = gate.parent_id.as_deref().and_then(|pid| {
                let raw = pid.strip_prefix("gate-")?.parse::<usize>().ok()?;
                id_to_index.get(&format!("gate-{}", raw + 1)).copied()
            });
            let index = self.push_gate(name, kind, parent, x_idx, y_idx);
            id_to_index.insert(gate.id.clone(), index);
            added += 1;
        }
        self.recompute_populations();
        if added == 0 {
            return Err(format!(
                "No gates could be imported from {} ({} warning(s) skipped)",
                path.display(),
                warnings.len()
            ));
        }
        if warnings.is_empty() {
            self.set_status(format!("Imported {added} Gating-ML gate(s)"));
        } else {
            self.set_status(format!(
                "Imported {added} Gating-ML gate(s), skipped {} (see status/details)",
                warnings.len()
            ));
        }
        Ok(())
    }

    /// Create a quadrant / bilinear gate at a click position.
    ///
    /// `popout` selects which graph window (the main plot or one detached
    /// window) the click came from, so the new gate gets the right parent and
    /// parameter pair.
    fn create_click_gate(&mut self, p: [f64; 2], popout: Option<usize>) {
        let (x_idx, y_idx, parent, ty, span) = if let Some(idx) = popout {
            let Some(st) = self.popouts.get(idx) else {
                return;
            };
            let (Some(x), Some(y)) = (st.x_param, st.y_param) else {
                return;
            };
            let span = (st.x_range.1 - st.x_range.0)
                .abs()
                .max((st.y_range.1 - st.y_range.0).abs());
            (x, y, st.gate, st.new_gate_type, span)
        } else {
            let (Some(x), Some(y)) = (self.x_param, self.y_param) else {
                return;
            };
            let span = (self.x_range.1 - self.x_range.0)
                .abs()
                .max((self.y_range.1 - self.y_range.0).abs());
            (x, y, self.selected_gate, self.new_gate_type, span)
        };
        let p = self.snap_drawing_point(p, x_idx, y_idx, span);
        let created = match ty {
            NewGateType::Quadrant => self.create_quadrant_at(p, x_idx, y_idx, parent),
            NewGateType::Bilinear => self.create_bilinear_at(p, span / 2.0, x_idx, y_idx, parent),
            _ => return,
        };
        if let Some(&last) = created.last() {
            if let Some(idx) = popout {
                if let Some(st) = self.popouts.get_mut(idx) {
                    st.selected_gate = Some(last);
                }
            } else {
                self.selected_gate = Some(last);
            }
        }
        self.finish_gate_creation();
    }

    /// Open a detached graph window for a population and prime its view
    /// controls from the main plot. Reuses an existing window for the same
    /// population so the tree button behaves as "focus", not "spawn more".
    fn open_popout(&mut self, gate_idx: usize) {
        let Some(gate) = self.gates.get(gate_idx) else {
            return;
        };
        let (x, y) = (gate.x_idx, gate.y_idx);
        if let Some(i) = self.popouts.iter().position(|st| st.gate == Some(gate_idx)) {
            let mut st = std::mem::take(&mut self.popouts[i]);
            st.open = true;
            st.x_param = Some(x);
            st.y_param = Some(y);
            st.selected_gate = None;
            st.interact = Interact::default();
            self.popout_auto_range(&mut st);
            self.popouts[i] = st;
            self.active_popout = Some(i);
            return;
        }
        let mut st = PopoutState {
            open: true,
            gate: Some(gate_idx),
            x_param: Some(x),
            y_param: Some(y),
            view: self.view_mode,
            x_scale: self.x_scale.clone(),
            y_scale: self.y_scale.clone(),
            point_color: self.point_color,
            gradient: self.gradient,
            density_map: self.density_map,
            contour_levels: self.contour_levels,
            new_gate_type: self.new_gate_type,
            ..Default::default()
        };
        self.popout_auto_range(&mut st);
        self.popouts.push(st);
        self.active_popout = Some(self.popouts.len() - 1);
    }

    /// Duplicate a graph window with an independent plot spec (V7).
    fn duplicate_popout(&mut self, st: &PopoutState) {
        let mut copy = st.clone();
        copy.open = true;
        copy.selected_gate = None;
        copy.interact = Interact::default();
        self.popouts.push(copy);
        self.active_popout = Some(self.popouts.len() - 1);
        self.set_status("Duplicated graph window");
    }

    /// Duplicate the currently focused graph window.
    fn duplicate_active_popout(&mut self) {
        let idx = self
            .active_popout
            .filter(|&i| self.popouts.get(i).is_some_and(|st| st.open))
            .or_else(|| self.popouts.iter().position(|st| st.open));
        if let Some(i) = idx {
            let st = self.popouts[i].clone();
            self.duplicate_popout(&st);
        } else {
            self.set_status("Open a graph window first");
        }
    }

    /// Move to the previous/next sample in every graph window.
    fn step_sample(&mut self, delta: isize) {
        let n = self.samples.len();
        if n == 0 {
            return;
        }
        let cur = self.current_sample.unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(n as isize) as usize;
        self.current_sample = Some(next);
        self.update_plot_points();
        for i in 0..self.popouts.len() {
            let mut st = std::mem::take(&mut self.popouts[i]);
            self.popout_auto_range(&mut st);
            self.popouts[i] = st;
        }
        self.set_status(format!(
            "Sample {}/{}: {}",
            next + 1,
            n,
            self.samples[next].id
        ));
    }

    /// Move a graph window to its parent, first child or next sibling.
    fn navigate_popout(&mut self, st: &mut PopoutState, nav: PopNav) {
        let Some(gate) = st.gate else {
            return;
        };
        let target = match nav {
            PopNav::Parent => self.gates.get(gate).and_then(|g| g.parent),
            PopNav::FirstChild => self.children.get(gate).and_then(|c| c.first().copied()),
            PopNav::NextSibling => {
                let parent = self.gates.get(gate).and_then(|g| g.parent);
                let siblings: Vec<usize> = self
                    .gates
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| g.parent == parent)
                    .map(|(i, _)| i)
                    .collect();
                if siblings.len() <= 1 {
                    None
                } else {
                    let pos = siblings.iter().position(|&i| i == gate).unwrap_or(0);
                    Some(siblings[(pos + 1) % siblings.len()])
                }
            }
        };
        if let Some(target) = target {
            st.gate = Some(target);
            st.selected_gate = None;
            self.popout_auto_range(st);
        } else {
            self.set_status("No such population");
        }
    }

    /// Cycle the active graph window's X or Y parameter (V7).
    fn cycle_plot_param(&mut self, x_axis: bool, delta: isize) {
        let n = self.current_fcs().map(|f| f.parameters.len()).unwrap_or(0);
        if n == 0 {
            return;
        }
        let target = self
            .active_popout
            .filter(|&i| self.popouts.get(i).is_some_and(|st| st.open));
        if let Some(i) = target {
            let mut st = std::mem::take(&mut self.popouts[i]);
            if x_axis {
                st.x_param = cycle_param(st.x_param, n, delta);
            } else {
                st.y_param = cycle_param(st.y_param, n, delta);
            }
            self.popout_auto_range(&mut st);
            self.popouts[i] = st;
        } else if x_axis {
            self.x_param = cycle_param(self.x_param, n, delta);
            self.update_plot_points();
        } else {
            self.y_param = cycle_param(self.y_param, n, delta);
            self.update_plot_points();
        }
    }

    /// Auto-fit a graph window's ranges to its population.
    fn popout_auto_range(&mut self, st: &mut PopoutState) {
        let Some(gate_idx) = st.gate else {
            return;
        };
        let (Some(x), Some(y)) = (st.x_param, st.y_param) else {
            return;
        };
        let pts = self.population_points_xy(gate_idx, x, y);
        let xs: Vec<f64> = pts.iter().map(|p| p[0]).collect();
        let ys: Vec<f64> = pts.iter().map(|p| p[1]).collect();
        st.x_range = bounds_for(&xs, &st.x_scale);
        st.y_range = bounds_for(&ys, &st.y_scale);
    }

    /// Points of a population for an explicit parameter pair.
    fn population_points_xy(&self, gate_idx: usize, x: usize, y: usize) -> Vec<[f64; 2]> {
        let Some(fcs) = self.current_fcs() else {
            return Vec::new();
        };
        let (_, n_params) = fcs.data.dim();
        if x >= n_params || y >= n_params {
            return Vec::new();
        }
        let Some(idxs) = self.pop_indices.get(gate_idx) else {
            return Vec::new();
        };
        idxs.iter()
            .map(|&e| [fcs.data[[e, x]] as f64, fcs.data[[e, y]] as f64])
            .collect()
    }

    /// Controls for the detached population window. Mirrors the main window so
    /// every plotting feature (axes, scales, ranges, colour, view, gating and
    /// gate manipulation) is available there too.
    fn popout_controls(
        &mut self,
        ui: &mut egui::Ui,
        p: Palette,
        gate_idx: usize,
        st: &mut PopoutState,
        idx: usize,
    ) {
        let names: Vec<String> = match self.current_fcs() {
            Some(f) => (0..f.parameters.len())
                .map(|i| param_display_name(f, i))
                .collect(),
            None => Vec::new(),
        };
        let subset_len = self.pop_indices.get(gate_idx).map(|v| v.len()).unwrap_or(0);

        // ----- Graph-window navigation (V7) -----
        let has_parent = self.gates.get(gate_idx).is_some_and(|g| g.parent.is_some());
        let has_children = self
            .children
            .get(gate_idx)
            .map(|c| !c.is_empty())
            .unwrap_or(false);
        let sibling_count = self
            .gates
            .get(gate_idx)
            .map(|g| {
                self.gates
                    .iter()
                    .filter(|other| other.parent == g.parent)
                    .count()
            })
            .unwrap_or(0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Graph:").small().color(p.muted));
            if secondary_button(ui, p, "◀ Sample")
                .on_hover_text("Previous sample (Alt+PageUp)")
                .clicked()
            {
                self.step_sample(-1);
            }
            if secondary_button(ui, p, "Sample ▶")
                .on_hover_text("Next sample (Alt+PageDown)")
                .clicked()
            {
                self.step_sample(1);
            }
            ui.separator();
            if ui
                .add_enabled(
                    has_parent,
                    egui::Button::new(egui::RichText::new("Parent").small()),
                )
                .on_hover_text("Parent population (Cmd+[)")
                .clicked()
            {
                self.navigate_popout(st, PopNav::Parent);
            }
            if ui
                .add_enabled(
                    has_children,
                    egui::Button::new(egui::RichText::new("Child").small()),
                )
                .on_hover_text("First child population (Cmd+])")
                .clicked()
            {
                self.navigate_popout(st, PopNav::FirstChild);
            }
            if ui
                .add_enabled(
                    sibling_count > 1,
                    egui::Button::new(egui::RichText::new("Sibling").small()),
                )
                .on_hover_text("Next sibling population (Cmd+Shift+])")
                .clicked()
            {
                self.navigate_popout(st, PopNav::NextSibling);
            }
            ui.separator();
            if secondary_button(ui, p, "Duplicate window")
                .on_hover_text("Duplicate this graph window (Cmd+Shift+D)")
                .clicked()
            {
                self.duplicate_popout(st);
            }
        });

        ui.horizontal(|ui| {
            let xt = st
                .x_param
                .and_then(|i| names.get(i).cloned())
                .unwrap_or_else(|| "Select X".to_string());
            if ui
                .small_button("◀")
                .on_hover_text("Previous X parameter (Alt+Left)")
                .clicked()
            {
                st.x_param = cycle_param(st.x_param, names.len(), -1);
                self.popout_auto_range(st);
            }
            egui::ComboBox::new(("popout_x", idx), "X")
                .selected_text(xt)
                .show_ui(ui, |ui| {
                    for (i, name) in names.iter().enumerate() {
                        ui.selectable_value(&mut st.x_param, Some(i), name.clone());
                    }
                });
            if ui
                .small_button("▶")
                .on_hover_text("Next X parameter (Alt+Right)")
                .clicked()
            {
                st.x_param = cycle_param(st.x_param, names.len(), 1);
                self.popout_auto_range(st);
            }
            let yt = st
                .y_param
                .and_then(|i| names.get(i).cloned())
                .unwrap_or_else(|| "Select Y".to_string());
            if ui
                .small_button("◀")
                .on_hover_text("Previous Y parameter (Alt+Down)")
                .clicked()
            {
                st.y_param = cycle_param(st.y_param, names.len(), -1);
                self.popout_auto_range(st);
            }
            egui::ComboBox::new(("popout_y", idx), "Y")
                .selected_text(yt)
                .show_ui(ui, |ui| {
                    for (i, name) in names.iter().enumerate() {
                        ui.selectable_value(&mut st.y_param, Some(i), name.clone());
                    }
                });
            if ui
                .small_button("▶")
                .on_hover_text("Next Y parameter (Alt+Up)")
                .clicked()
            {
                st.y_param = cycle_param(st.y_param, names.len(), 1);
                self.popout_auto_range(st);
            }
            if secondary_button(ui, p, "Auto range").clicked() {
                self.popout_auto_range(st);
            }
            ui.label(
                egui::RichText::new(format!("{subset_len} events"))
                    .small()
                    .color(p.muted),
            );
        });

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("X scale:").small().color(p.muted));
            for scale in AxisScale::all() {
                if ui
                    .selectable_label(st.x_scale == scale, scale.label())
                    .clicked()
                {
                    st.x_scale = scale;
                    self.popout_auto_range(st);
                }
            }
            ui.label(egui::RichText::new("range:").small().color(p.muted));
            let span = (st.x_range.1 - st.x_range.0).abs().max(1.0);
            ui.add(range_drag_value(&mut st.x_range.0, span));
            ui.add(range_drag_value(&mut st.x_range.1, span));
            if ui.small_button("−").on_hover_text("Zoom X out").clicked() {
                zoom_axis_range(&mut st.x_range, &st.x_scale, 1.25);
            }
            if ui.small_button("+").on_hover_text("Zoom X in").clicked() {
                zoom_axis_range(&mut st.x_range, &st.x_scale, 0.8);
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Y scale:").small().color(p.muted));
            for scale in AxisScale::all() {
                if ui
                    .selectable_label(st.y_scale == scale, scale.label())
                    .clicked()
                {
                    st.y_scale = scale;
                    self.popout_auto_range(st);
                }
            }
            ui.label(egui::RichText::new("range:").small().color(p.muted));
            let span = (st.y_range.1 - st.y_range.0).abs().max(1.0);
            ui.add(range_drag_value(&mut st.y_range.0, span));
            ui.add(range_drag_value(&mut st.y_range.1, span));
            if ui.small_button("−").on_hover_text("Zoom Y out").clicked() {
                zoom_axis_range(&mut st.y_range, &st.y_scale, 1.25);
            }
            if ui.small_button("+").on_hover_text("Zoom Y in").clicked() {
                zoom_axis_range(&mut st.y_range, &st.y_scale, 0.8);
            }
        });

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Dot colour:").small().color(p.muted));
            ui.color_edit_button_srgba(&mut st.point_color);
            ui.checkbox(&mut st.gradient, "Gradient");
            ui.label(egui::RichText::new("View:").small().color(p.muted));
            for mode in [
                ViewMode::Dots,
                ViewMode::Contour,
                ViewMode::Histogram,
                ViewMode::Pseudocolor,
                ViewMode::Zebra,
            ] {
                if ui.selectable_label(st.view == mode, mode.label()).clicked() {
                    st.view = mode;
                }
            }
            if matches!(st.view, ViewMode::Contour) {
                ui.add(egui::Slider::new(&mut st.contour_levels, 3..=100).text("levels"));
            }
            if matches!(st.view, ViewMode::Pseudocolor | ViewMode::Zebra) {
                ui.label(egui::RichText::new("Map:").small().color(p.muted));
                egui::ComboBox::new(("popout_density_map", idx), "")
                    .selected_text(st.density_map.label())
                    .show_ui(ui, |ui| {
                        for map in crate::plot::DensityMap::all() {
                            ui.selectable_value(&mut st.density_map, map, map.label());
                        }
                    });
            }
        });

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("New gate:").small().color(p.muted));
            for gt in NewGateType::ALL {
                ui.radio_value(&mut st.new_gate_type, gt, gt.label());
            }
        });

        // Manipulation buttons for the gate selected inside the popout.
        if let Some(gi) = st.selected_gate
            && gi < self.gates.len()
        {
            let span = (st.x_range.1 - st.x_range.0)
                .abs()
                .max((st.y_range.1 - st.y_range.0).abs())
                .max(1.0);
            let step = span * 0.02;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Transform:").small().color(p.muted));
                let mut t = false;
                if secondary_button(ui, p, "⬅").clicked() {
                    self.nudge_gate(gi, -step, 0.0);
                    t = true;
                }
                if secondary_button(ui, p, "➡").clicked() {
                    self.nudge_gate(gi, step, 0.0);
                    t = true;
                }
                if secondary_button(ui, p, "⬆").clicked() {
                    self.nudge_gate(gi, 0.0, step);
                    t = true;
                }
                if secondary_button(ui, p, "⬇").clicked() {
                    self.nudge_gate(gi, 0.0, -step);
                    t = true;
                }
                if secondary_button_hover(ui, p, "⟲", "Rotate -5°").clicked() {
                    let c = self.gates[gi].kind.center();
                    self.rotate_gate(gi, c, (-5.0f64).to_radians());
                    t = true;
                }
                if secondary_button_hover(ui, p, "⟳", "Rotate +5°").clicked() {
                    let c = self.gates[gi].kind.center();
                    self.rotate_gate(gi, c, (5.0f64).to_radians());
                    t = true;
                }
                if secondary_button_hover(ui, p, "➕", "Enlarge").clicked() {
                    let c = self.gates[gi].kind.center();
                    self.scale_gate(gi, c, 1.1);
                    t = true;
                }
                if secondary_button_hover(ui, p, "➖", "Shrink").clicked() {
                    let c = self.gates[gi].kind.center();
                    self.scale_gate(gi, c, 0.9);
                    t = true;
                }
                if t {
                    self.recompute_populations();
                }
            });
        }
    }

    // -- rendering helpers --------------------------------------------------

    /// Gate overlays (in data coordinates) for the active X/Y pair.
    fn gate_visuals(&self) -> Vec<GateVisual> {
        let (Some(x), Some(y)) = (self.x_param, self.y_param) else {
            return Vec::new();
        };
        self.gate_visuals_for(x, y, self.x_range, self.y_range, self.selected_gate)
    }

    /// Gate overlays for an explicit parameter pair and axis ranges.
    ///
    /// When population labels are enabled each visible gate gets a label with
    /// its name/count/percentages at the polygon centroid. Backgating marks the
    /// ancestors of the selected gate so they can be drawn as dashed outlines.
    fn gate_visuals_for(
        &self,
        x: usize,
        y: usize,
        x_range: (f64, f64),
        y_range: (f64, f64),
        selected: Option<usize>,
    ) -> Vec<GateVisual> {
        let parents: Vec<Option<usize>> = self.gates.iter().map(|g| g.parent).collect();
        let backgated: std::collections::HashSet<usize> = if self.gate_labels.backgating {
            selected
                .map(|s| backgate_ancestors(&parents, s).into_iter().collect())
                .unwrap_or_default()
        } else {
            std::collections::HashSet::new()
        };
        let total_count = self.current_fcs().map(|f| f.event_count).unwrap_or(0);
        let mut out = Vec::new();
        for (i, gate) in self.gates.iter().enumerate() {
            if gate.x_idx != x || gate.y_idx != y {
                continue;
            }
            let median = if self.show_median_cross {
                self.current_fcs()
                    .and_then(|fcs| self.pop_indices.get(i).map(|idx| (fcs, idx)))
                    .and_then(|(fcs, idx)| crate::gating::median_cross(&fcs.data, idx, x, y))
                    .map(|(mx, my)| [mx, my])
            } else {
                None
            };
            let fill = gate.kind.polygon(x_range, y_range);
            let label = if self.gate_labels.show {
                let count = self.pop_indices.get(i).map(|v| v.len()).unwrap_or(0);
                let parent_count = gate
                    .parent
                    .and_then(|p| self.pop_indices.get(p).map(|v| v.len()));
                let mut ancestry: Vec<String> = backgate_ancestors(&parents, i)
                    .into_iter()
                    .filter_map(|idx| self.gates.get(idx).map(|g| g.name.clone()))
                    .collect();
                ancestry.push(gate.name.clone());
                let info = PopulationLabel {
                    name: gate.name.clone(),
                    count,
                    parent_count,
                    total_count,
                    ancestry,
                };
                let text = info.text(&self.gate_labels);
                if text.is_empty() {
                    None
                } else {
                    polygon_centroid(&fill).map(|position| (text, position))
                }
            } else {
                None
            };
            out.push(GateVisual {
                fill,
                lines: gate.kind.divider_lines(x_range, y_range),
                color: gate.color,
                selected: selected == Some(i),
                line_width: self.gate_line_width,
                median,
                label,
                backgate: backgated.contains(&i),
            });
        }
        out
    }

    /// Build the plot configuration for the main view.
    /// Events in the parent population that fall outside the selected gate,
    /// i.e. the events that would be included if the gate were omitted (V4).
    fn backgate_residual_points(&self) -> Vec<[f64; 2]> {
        let (Some(sel), Some(fcs), Some(x), Some(y)) = (
            self.selected_gate,
            self.current_fcs(),
            self.x_param,
            self.y_param,
        ) else {
            return Vec::new();
        };
        let Some(gate) = self.gates.get(sel) else {
            return Vec::new();
        };
        if gate.x_idx != x || gate.y_idx != y {
            return Vec::new();
        }
        let Some(current) = self.pop_indices.get(sel) else {
            return Vec::new();
        };
        let parent: Vec<usize> = match gate.parent {
            Some(p) => self.pop_indices.get(p).cloned().unwrap_or_default(),
            None => (0..fcs.event_count).collect(),
        };
        parent
            .iter()
            .filter(|&&e| current.binary_search(&e).is_err())
            .map(|&e| [fcs.data[[e, x]] as f64, fcs.data[[e, y]] as f64])
            .collect()
    }

    /// Build data overlays for every other population gated on the current
    /// X/Y parameter pair (V2). Returns an empty list when overlays are off.
    fn data_overlays_for_plot(&self) -> Vec<crate::plot::DataOverlay> {
        let mut overlays = Vec::new();
        if self.gate_labels.backgate_events {
            let residual = self.backgate_residual_points();
            if !residual.is_empty() {
                let label = self
                    .selected_gate
                    .and_then(|i| self.gates.get(i))
                    .map(|g| format!("Excluded by {}", g.name))
                    .unwrap_or_else(|| "Excluded events".to_string());
                overlays.push(crate::plot::DataOverlay::dots(label, "#94a3b8", residual));
            }
        }
        if !self.overlay_populations {
            return overlays;
        }
        let (Some(fcs), Some(x), Some(y)) = (self.current_fcs(), self.x_param, self.y_param) else {
            return overlays;
        };
        for (i, gate) in self.gates.iter().enumerate() {
            if gate.x_idx != x || gate.y_idx != y || Some(i) == self.selected_gate {
                continue;
            }
            let Some(indices) = self.pop_indices.get(i) else {
                continue;
            };
            if indices.is_empty() {
                continue;
            }
            let points = indices
                .iter()
                .map(|&e| [fcs.data[[e, x]] as f64, fcs.data[[e, y]] as f64])
                .collect();
            overlays.push(crate::plot::DataOverlay::dots(
                gate.name.clone(),
                color_to_hex(gate_color(i)),
                points,
            ));
        }
        overlays
    }

    /// Render a multigraph sheet for the current sample and parameters (V2).
    fn multigraph_svg_for(
        &self,
        mode: crate::plot::MultigraphMode,
        columns: usize,
    ) -> Result<(String, u32, u32), String> {
        let Some(fcs) = self.current_fcs() else {
            return Err("No sample loaded to render a multigraph".to_string());
        };
        let (Some(x), Some(y)) = (self.x_param, self.y_param) else {
            return Err("Select X and Y parameters to render a multigraph".to_string());
        };
        let labels: Vec<String> = fcs
            .parameters
            .iter()
            .enumerate()
            .map(|(i, _)| param_display_name(fcs, i))
            .collect();
        let panels = crate::plot::multigraph_panels(fcs.parameters.len(), x, y, mode);
        if panels.is_empty() {
            return Err("No multigraph panels to render".to_string());
        }
        let panel_w = 280u32;
        let panel_h = 220u32;
        let style = PlotStyle {
            point_color: color_to_hex(self.point_color),
            x_scale: AxisScale::Linear,
            y_scale: AxisScale::Linear,
            gradient: self.gradient,
            ..Default::default()
        };
        let svg = crate::plot::multigraph_svg(
            &fcs.data, &panels, columns, panel_w, panel_h, &labels, &style,
        );
        let columns = columns.max(1);
        let rows = panels.len().div_ceil(columns);
        let total_w = columns as u32 * panel_w + (columns as u32 + 1) * 8;
        let total_h = rows as u32 * panel_h + (rows as u32 + 1) * 8;
        Ok((svg, total_w, total_h))
    }

    /// Ask for a path and export a multigraph sheet as SVG.
    fn export_multigraph(&mut self, mode: crate::plot::MultigraphMode, columns: usize) {
        let (default_name, label) = match mode {
            crate::plot::MultigraphMode::AllByX => ("multigraph_by_x.svg", "all-by-X"),
            crate::plot::MultigraphMode::AllByY => ("multigraph_by_y.svg", "all-by-Y"),
            crate::plot::MultigraphMode::NxN => ("multigraph_nxn.svg", "NxN"),
        };
        match self.multigraph_svg_for(mode, columns) {
            Ok((svg, w, h)) => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("SVG", &["svg"])
                    .set_file_name(default_name)
                    .save_file()
                {
                    match std::fs::write(&path, svg) {
                        Ok(()) => self.set_status(format!(
                            "Exported {label} multigraph ({w}x{h}) to {}",
                            path.display()
                        )),
                        Err(e) => self.set_status(format!("Multigraph export failed: {e}")),
                    }
                }
            }
            Err(e) => self.set_status(e),
        }
    }

    /// Applies axis transposition to a fully-built plot configuration (V5).
    ///
    /// Swapping at the presentation layer keeps gate geometry, labels and data
    /// overlays consistent. Gate editing is disabled while transposed because
    /// pointer coordinates no longer match the gate's native X/Y pair.
    fn apply_transpose_to_cfg(&self, cfg: &mut PlotCfg) {
        if !self.transpose || matches!(cfg.view, ViewMode::Histogram) {
            return;
        }
        std::mem::swap(&mut cfg.x_range, &mut cfg.y_range);
        std::mem::swap(&mut cfg.x_scale, &mut cfg.y_scale);
        std::mem::swap(&mut cfg.x_label, &mut cfg.y_label);
        for p in &mut cfg.points {
            *p = [p[1], p[0]];
        }
        for overlay in &mut cfg.data_overlays {
            for p in &mut overlay.points {
                *p = [p[1], p[0]];
            }
        }
        for gate in &mut cfg.gates {
            for p in &mut gate.fill {
                *p = [p[1], p[0]];
            }
            for line in &mut gate.lines {
                for p in line {
                    *p = [p[1], p[0]];
                }
            }
            if let Some(median) = gate.median.as_mut() {
                *median = [median[1], median[0]];
            }
            if let Some((_, position)) = gate.label.as_mut() {
                *position = [position[1], position[0]];
            }
        }
        cfg.handles.clear();
        cfg.interactive = false;
        cfg.transposed = true;
    }

    fn main_plot_cfg(&self) -> PlotCfg {
        let x = self.x_param.unwrap_or(0);
        let y = self.y_param.unwrap_or(0);
        let x_label = self
            .current_fcs()
            .and_then(|f| self.x_param.map(|x| param_display_name(f, x)))
            .unwrap_or_default();
        let y_label = self
            .current_fcs()
            .and_then(|f| self.y_param.map(|y| param_display_name(f, y)))
            .unwrap_or_default();
        let handles = match self.selected_gate {
            Some(i) if self.gates.get(i).map(|g| (g.x_idx, g.y_idx)) == Some((x, y)) => {
                self.gates[i].kind.handles()
            }
            _ => Vec::new(),
        };
        let cfg = PlotCfg {
            points: self.plot_points.clone(),
            x_range: self.x_range,
            y_range: self.y_range,
            x_scale: self.x_scale.clone(),
            y_scale: self.y_scale.clone(),
            point_color: self.point_color,
            gradient: self.gradient,
            view: self.view_mode,
            density_map: self.density_map,
            data_overlays: self.data_overlays_for_plot(),
            contour_levels: self.contour_levels,
            gates: self.gate_visuals(),
            handles,
            x_label: format!("{} ({})", x_label, scale_label(&self.x_scale)),
            y_label: if matches!(self.view_mode, ViewMode::Histogram) {
                "Count".to_string()
            } else {
                format!("{} ({})", y_label, scale_label(&self.y_scale))
            },
            interactive: true,
            line_preview: matches!(self.new_gate_type, NewGateType::Bilinear),
            smoothing: self.plot_smoothing,
            resolution: self.plot_resolution,
            outlier_clip_percent: self.outlier_clip_percent,
            large_dots: self.large_dots,
            grid: self.show_grid,
            medians: self.show_medians,
            show_event_numbers: self.show_event_numbers,
            show_names_frequencies: self.show_names_frequencies,
            notation: self.tick_notation,
            show_axis_labels: self.show_axis_labels,
            show_ticks: self.show_ticks,
            show_numbers: self.show_numbers,
            axis_histograms: self.show_axis_histograms,
            transposed: false,
        };
        let mut cfg = cfg;
        self.apply_transpose_to_cfg(&mut cfg);
        cfg
    }

    /// Build the SVG for the current main plot, with gate overlays.
    /// Returns the SVG text plus its logical `(width, height)`.
    fn current_plot_svg(&self) -> Result<(String, u32, u32), String> {
        let Some(fcs) = self.current_fcs() else {
            return Err("No sample loaded to export".to_string());
        };
        let (Some(x), Some(y)) = (self.x_param, self.y_param) else {
            return Err("Select X and Y parameters to export".to_string());
        };

        // Build the same presentation configuration as the interactive plot so
        // graph options (including axis transposition) apply to exports too.
        let cfg = self.main_plot_cfg();
        let (ex, ey) = if cfg.transposed { (y, x) } else { (x, y) };

        let overlays: Vec<PlotOverlay> = cfg
            .gates
            .iter()
            .map(|g| PlotOverlay {
                points: g.fill.clone(),
                stroke: color_to_hex(g.color),
                fill: None,
                closed: true,
                label: g.label.as_ref().map(|(text, position)| OverlayLabel {
                    text: text.clone(),
                    position: *position,
                    color: color_to_hex(g.color),
                    size: 11.0,
                }),
                dashed: g.backgate,
            })
            .collect();
        let data_overlays = cfg.data_overlays.clone();

        let style = PlotStyle {
            point_color: color_to_hex(self.point_color),
            x_scale: cfg.x_scale.clone(),
            y_scale: cfg.y_scale.clone(),
            gradient: self.gradient,
            contour: matches!(self.view_mode, ViewMode::Contour),
            contour_levels: self.contour_levels,
            title: param_display_name(fcs, y),
            line_color: color_to_hex(egui::Color32::from_rgb(44, 160, 44)),
            smoothing: self.plot_smoothing,
            resolution: self.plot_resolution,
            outlier_clip_percent: self.outlier_clip_percent,
            large_dots: self.large_dots,
            grid: self.show_grid,
            medians: self.show_medians,
            show_event_numbers: self.show_event_numbers,
            show_names_frequencies: self.show_names_frequencies,
            notation: self.tick_notation,
            show_axis_labels: self.show_axis_labels,
            show_ticks: self.show_ticks,
            show_numbers: self.show_numbers,
            axis_histograms: self.show_axis_histograms,
            show_scale_labels: false,
            ..Default::default()
        };

        let svg = if matches!(self.view_mode, ViewMode::Histogram) {
            crate::plot::histogram_svg(
                &fcs.data,
                x,
                &cfg.x_label,
                "Count",
                &[],
                Some((cfg.x_range.0 as f32, cfg.x_range.1 as f32)),
                400,
                &style,
            )
        } else {
            let density = matches!(self.view_mode, ViewMode::Pseudocolor | ViewMode::Zebra)
                .then_some((&self.density_map, 8usize));
            crate::plot::dot_plot_svg_multi(
                &fcs.data,
                ex,
                ey,
                &cfg.x_label,
                &cfg.y_label,
                &[],
                Some((cfg.x_range.0 as f32, cfg.x_range.1 as f32)),
                Some((cfg.y_range.0 as f32, cfg.y_range.1 as f32)),
                &overlays,
                &data_overlays,
                &style,
                density,
            )
        };
        Ok((svg, style.width, style.height))
    }

    /// Export the current plot (with gates) to SVG.
    fn export_svg(&mut self, path: &std::path::Path) {
        match self.current_plot_svg() {
            Ok((svg, _, _)) => match std::fs::write(path, &svg) {
                Ok(()) => self.set_status(format!("Exported plot to {}", path.display())),
                Err(e) => self.set_status(format!("Export failed: {e}")),
            },
            Err(e) => self.set_status(e),
        }
    }

    /// Export the current plot to a raster format (PNG / JPEG / TIFF).
    fn export_image(&mut self, path: &std::path::Path) {
        match self.current_plot_svg() {
            Ok((svg, w, h)) => {
                let scale = self.export_scale;
                match save_svg_as_image(&svg, path, w, h, scale) {
                    Ok(fmt) => self.set_status(format!(
                        "Exported {} ({}×{}, {}×) to {}",
                        fmt.label(),
                        (w as f32 * scale).round() as u32,
                        (h as f32 * scale).round() as u32,
                        scale,
                        path.display()
                    )),
                    Err(e) => self.set_status(format!("Image export failed: {e}")),
                }
            }
            Err(e) => self.set_status(e),
        }
    }

    /// Collect population rows across *all* loaded samples.
    fn population_rows(&self) -> Vec<PopulationRow> {
        let mut rows = Vec::new();
        for (sample_idx, sample) in self.samples.iter().enumerate() {
            let pops = compute_gate_populations(
                &sample.fcs.data,
                &self.gates,
                Some(sample_idx),
                sample.apply_gates,
            );
            for (i, gate) in self.gates.iter().enumerate() {
                if i >= pops.len() {
                    continue;
                }
                rows.push(PopulationRow {
                    sample: sample.id.clone(),
                    population: gate.name.clone(),
                    parent: gate.parent.map(|p| self.gates[p].name.clone()),
                    count: pops[i].len(),
                    total: sample.fcs.event_count,
                    parent_count: gate.parent.and_then(|p| pops.get(p).map(|v| v.len())),
                    gate_summary: Some(gate.kind.summary()),
                });
            }
        }
        rows
    }

    fn export_csv(&mut self, path: &std::path::Path) {
        let rows = self.population_rows();
        match write_populations_csv(path, &rows) {
            Ok(()) => self.set_status(format!(
                "Exported {} population rows to {}",
                rows.len(),
                path.display()
            )),
            Err(e) => self.set_status(format!("CSV export failed: {e}")),
        }
    }

    /// Build the panel SVGs and export the layout figure.
    /// Compose the layout canvas panels into a single SVG document.
    fn layout_svg(&self) -> (String, usize) {
        // Each panel reserves a title strip above its content, so the canvas
        // height must include it or the bottom axis labels get clipped.
        let title_h = LayoutOptions::default().title_height as f32;
        let mut max_x = 100.0f32;
        let mut max_y = 100.0f32;
        for p in &self.layout_panels {
            max_x = max_x.max(p.rect.max.x);
            max_y = max_y.max(p.rect.max.y + title_h);
        }
        let canvas_w = (max_x + 20.0) as u32;
        let canvas_h = (max_y + 20.0) as u32;

        let placements: Vec<LayoutPlacement> = self
            .layout_panels
            .iter()
            .filter_map(|spec| {
                let sample = self.samples.get(spec.sample)?;
                let fcs = &sample.fcs;
                let x_text = spec
                    .x_label
                    .text
                    .clone()
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| param_display_name(fcs, spec.x));
                let y_text = spec
                    .y_label
                    .text
                    .clone()
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| {
                        if matches!(spec.view, ViewMode::Histogram) {
                            "Count".to_string()
                        } else {
                            param_display_name(fcs, spec.y)
                        }
                    });
                let style = PlotStyle {
                    point_color: color_to_hex(self.point_color),
                    x_scale: AxisScale::Linear,
                    y_scale: AxisScale::Linear,
                    gradient: self.gradient,
                    contour: matches!(spec.view, ViewMode::Contour),
                    contour_levels: self.contour_levels,
                    title: spec.title.clone(),
                    x_label_size: spec.x_label.size,
                    y_label_size: spec.y_label.size,
                    x_label_rotation: spec.x_label.rotation_deg,
                    y_label_rotation: spec.y_label.rotation_deg,
                    x_label_font: spec.x_label.font.css().to_string(),
                    y_label_font: spec.y_label.font.css().to_string(),
                    smoothing: self.plot_smoothing,
                    resolution: self.plot_resolution,
                    outlier_clip_percent: self.outlier_clip_percent,
                    large_dots: self.large_dots,
                    grid: self.show_grid,
                    medians: self.show_medians,
                    show_event_numbers: self.show_event_numbers,
                    show_names_frequencies: self.show_names_frequencies,
                    notation: self.tick_notation,
                    show_axis_labels: self.show_axis_labels,
                    show_ticks: self.show_ticks,
                    show_numbers: self.show_numbers,
                    axis_histograms: self.show_axis_histograms,
                    ..Default::default()
                };
                let svg = if matches!(spec.view, ViewMode::Histogram) {
                    crate::plot::histogram_svg(
                        &fcs.data,
                        spec.x,
                        &x_text,
                        &y_text,
                        &[],
                        None,
                        400,
                        &style,
                    )
                } else {
                    let indices: Vec<usize> = spec
                        .gate
                        .map(|g| {
                            compute_gate_populations(
                                &fcs.data,
                                &self.gates,
                                Some(spec.sample),
                                sample.apply_gates,
                            )
                            .get(g)
                            .cloned()
                            .unwrap_or_default()
                        })
                        .unwrap_or_default();
                    if matches!(spec.view, ViewMode::Pseudocolor | ViewMode::Zebra) {
                        crate::plot::dot_plot_svg_density(
                            &fcs.data,
                            spec.x,
                            spec.y,
                            &x_text,
                            &y_text,
                            &indices,
                            None,
                            None,
                            &[],
                            &style,
                            &self.density_map,
                            8,
                        )
                    } else {
                        dot_plot_svg(
                            &fcs.data,
                            spec.x,
                            spec.y,
                            &x_text,
                            &y_text,
                            &indices,
                            None,
                            None,
                            &[],
                            &style,
                        )
                    }
                };
                Some(LayoutPlacement {
                    title: spec.title.clone(),
                    svg,
                    svg_width: style.width,
                    svg_height: style.height,
                    x: spec.rect.min.x,
                    y: spec.rect.min.y,
                    width: spec.rect.width(),
                    height: spec.rect.height(),
                    title_size: spec.title_size,
                })
            })
            .collect();

        let svg = render_layout_free(canvas_w, canvas_h, &placements, &LayoutOptions::default());
        (svg, placements.len())
    }

    fn export_layout(&mut self, path: &std::path::Path) {
        let (svg, n) = self.layout_svg();
        match std::fs::write(path, &svg) {
            Ok(()) => self.set_status(format!("Exported {n} panel layout to {}", path.display())),
            Err(e) => self.set_status(format!("Layout export failed: {e}")),
        }
    }

    /// Export the composed layout to a raster format.
    fn export_layout_image(&mut self, path: &std::path::Path) {
        let (svg, n) = self.layout_svg();
        let (w, h) = layout_canvas_size(&self.layout_panels);
        match save_svg_as_image(&svg, path, w, h, self.export_scale) {
            Ok(fmt) => self.set_status(format!(
                "Exported {n}-panel layout as {} to {}",
                fmt.label(),
                path.display()
            )),
            Err(e) => self.set_status(format!("Layout image export failed: {e}")),
        }
    }

    /// Default plot title for a layout panel (used when the user clears it).
    fn default_panel_title(&self, panel: &LayoutPanelSpec) -> String {
        let Some(s) = self.samples.get(panel.sample) else {
            return String::new();
        };
        let fcs = &s.fcs;
        if let Some(g) = panel.gate {
            let gate_name = self
                .gates
                .get(g)
                .map(|g| g.name.clone())
                .unwrap_or_else(|| "population".to_string());
            let x_name = param_display_name(fcs, panel.x);
            let y_name = param_display_name(fcs, panel.y);
            return format!("Population: {gate_name} ({x_name} vs {y_name})");
        }
        param_display_name(fcs, panel.y)
    }

    /// Add the current main plot to the layout canvas.
    fn add_layout_current(&mut self) {
        let (Some(s), Some(x), Some(y)) = (self.current_sample, self.x_param, self.y_param) else {
            self.set_status("Select a sample and X/Y parameters first");
            return;
        };
        let title = param_display_name(&self.samples[s].fcs, y);
        let preview = self.make_layout_preview(s, x, y);
        let n = self.layout_panels.len();
        let cols = 3usize;
        let col = (n % cols) as f32;
        let row = (n / cols) as f32;
        let rect = egui::Rect::from_min_size(
            egui::pos2(20.0 + col * 300.0, 20.0 + row * 240.0),
            egui::vec2(280.0, 220.0),
        );
        self.layout_panels.push(LayoutPanelSpec {
            sample: s,
            x,
            y,
            view: self.view_mode,
            gate: None,
            title,
            title_size: 14.0,
            rect,
            preview,
            x_label: LabelStyle::x_default(),
            y_label: LabelStyle::y_default(),
        });
        self.layout_selected = Some(self.layout_panels.len() - 1);
        self.layout_open = true;
        self.set_status("Added panel to layout");
    }

    /// Add the focused (or first open) graph window to the layout.
    fn add_layout_popout_active(&mut self) {
        let idx = self
            .active_popout
            .filter(|&i| self.popouts.get(i).is_some_and(|st| st.open))
            .or_else(|| self.popouts.iter().position(|st| st.open));
        if let Some(i) = idx {
            let st = self.popouts[i].clone();
            self.add_layout_popout(&st);
        } else {
            self.set_status("Open a detached population window first");
        }
    }

    /// Add a detached graph window's current view to the layout canvas.
    fn add_layout_popout(&mut self, st: &PopoutState) {
        let (Some(s), Some(gate_idx), Some(x), Some(y)) =
            (self.current_sample, st.gate, st.x_param, st.y_param)
        else {
            self.set_status("Open a detached population window first");
            return;
        };
        let gate_name = self.gates[gate_idx].name.clone();
        let x_name = param_display_name(&self.samples[s].fcs, x);
        let y_name = param_display_name(&self.samples[s].fcs, y);
        let title = format!("Population: {gate_name} ({x_name} vs {y_name})");
        let preview = self.make_popout_preview(s, gate_idx, x, y);
        let n = self.layout_panels.len();
        let cols = 3usize;
        let col = (n % cols) as f32;
        let row = (n / cols) as f32;
        let rect = egui::Rect::from_min_size(
            egui::pos2(20.0 + col * 300.0, 20.0 + row * 240.0),
            egui::vec2(280.0, 220.0),
        );
        self.layout_panels.push(LayoutPanelSpec {
            sample: s,
            x,
            y,
            view: st.view,
            gate: Some(gate_idx),
            title,
            title_size: 14.0,
            rect,
            preview,
            x_label: LabelStyle::x_default(),
            y_label: LabelStyle::y_default(),
        });
        self.layout_selected = Some(self.layout_panels.len() - 1);
        self.layout_open = true;
        self.set_status("Added population panel to layout");
    }

    /// Compute normalised (0..1) preview points for a sample's parameter pair.
    fn make_layout_preview(&self, sample: usize, x: usize, y: usize) -> Vec<[f32; 2]> {
        self.make_preview_points(sample, None, x, y)
    }

    /// Compute normalised (0..1) preview points for a gated population.
    fn make_popout_preview(
        &self,
        sample: usize,
        gate_idx: usize,
        x: usize,
        y: usize,
    ) -> Vec<[f32; 2]> {
        self.make_preview_points(sample, Some(gate_idx), x, y)
    }

    /// Shared helper: normalised preview points for all events or a population.
    fn make_preview_points(
        &self,
        sample: usize,
        gate: Option<usize>,
        x: usize,
        y: usize,
    ) -> Vec<[f32; 2]> {
        let Some(s) = self.samples.get(sample) else {
            return Vec::new();
        };
        let fcs = &s.fcs;
        let (n, n_params) = fcs.data.dim();
        if x >= n_params || y >= n_params {
            return Vec::new();
        }

        // Collect the (x, y) values for all events or the gated subset.
        let pts: Vec<[f64; 2]> = match gate {
            Some(g) => {
                let pops =
                    compute_gate_populations(&fcs.data, &self.gates, Some(sample), s.apply_gates);
                let idxs = pops.get(g).cloned().unwrap_or_default();
                idxs.iter()
                    .map(|&e| [fcs.data[[e, x]] as f64, fcs.data[[e, y]] as f64])
                    .collect()
            }
            None => (0..n)
                .filter_map(|i| {
                    let px = fcs.data[[i, x]] as f64;
                    let py = fcs.data[[i, y]] as f64;
                    if px.is_finite() && py.is_finite() {
                        Some([px, py])
                    } else {
                        None
                    }
                })
                .collect(),
        };

        if pts.is_empty() {
            return Vec::new();
        }

        let stride = (pts.len() / 1500).max(1);
        let (mut xmin, mut xmax) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut ymin, mut ymax) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut out: Vec<[f32; 2]> = Vec::new();
        for (i, p) in pts.iter().enumerate() {
            if i % stride != 0 {
                continue;
            }
            xmin = xmin.min(p[0]);
            xmax = xmax.max(p[0]);
            ymin = ymin.min(p[1]);
            ymax = ymax.max(p[1]);
            out.push([p[0] as f32, p[1] as f32]);
        }
        let xspan = (xmax - xmin).max(f64::EPSILON);
        let yspan = (ymax - ymin).max(f64::EPSILON);
        out.into_iter()
            .map(|p| {
                [
                    ((p[0] as f64 - xmin) / xspan) as f32,
                    ((p[1] as f64 - ymin) / yspan) as f32,
                ]
            })
            .collect()
    }

    /// Arrange all layout panels in a tidy grid.
    fn tile_layout(&mut self) {
        let cols = self.layout_columns.max(1);
        let (w, h, pad) = (280.0f32, 220.0f32, 20.0f32);
        for (i, p) in self.layout_panels.iter_mut().enumerate() {
            let col = (i % cols) as f32;
            let row = (i / cols) as f32;
            p.rect = egui::Rect::from_min_size(
                egui::pos2(pad + col * (w + pad), pad + row * (h + pad)),
                egui::vec2(w, h),
            );
        }
    }

    /// The interactive layout canvas (drag to move, corner to resize).
    fn layout_canvas(&mut self, ui: &mut egui::Ui, p: Palette) {
        let canvas_size = egui::vec2(920.0, 560.0);
        let (response, painter) = ui.allocate_painter(canvas_size, egui::Sense::click_and_drag());
        let origin = response.rect.min;
        let bg = p.soft;
        painter.rect_filled(response.rect, 4.0_f32, bg);

        let pointer = ui.input(|i| i.pointer.interact_pos());
        let down = ui.input(|i| i.pointer.primary_down());

        // Begin / end a manipulation.
        if !down {
            self.layout_drag = None;
        }
        if down
            && self.layout_drag.is_none()
            && let Some(p) = pointer
        {
            // Only clear/select from canvas clicks; ignore clicks on
            // toolbar buttons (which lie outside the canvas rect).
            if response.rect.contains(p) {
                let mut hit: Option<(usize, bool)> = None;
                for (i, panel) in self.layout_panels.iter().enumerate().rev() {
                    let r = panel.rect.translate(origin.to_vec2());
                    let handle = egui::Rect::from_min_size(
                        r.max - egui::vec2(14.0, 14.0),
                        egui::vec2(14.0, 14.0),
                    );
                    if handle.contains(p) {
                        hit = Some((i, true));
                        break;
                    }
                    if r.contains(p) {
                        hit = Some((i, false));
                        break;
                    }
                }
                match hit {
                    Some((i, resize)) => {
                        self.layout_selected = Some(i);
                        let r = self.layout_panels[i].rect.translate(origin.to_vec2());
                        let grab = if resize { egui::Vec2::ZERO } else { p - r.min };
                        self.layout_drag = Some(LayoutDrag {
                            panel: i,
                            resize,
                            grab,
                        });
                    }
                    None => self.layout_selected = None,
                }
            }
        }
        // Apply the manipulation.
        if let Some(drag) = self.layout_drag
            && let Some(p) = pointer
        {
            let local = egui::pos2(p.x - origin.x, p.y - origin.y);
            if let Some(panel) = self.layout_panels.get_mut(drag.panel) {
                if drag.resize {
                    let size = (local - panel.rect.min).max(egui::vec2(80.0, 60.0));
                    panel.rect = egui::Rect::from_min_max(panel.rect.min, panel.rect.min + size);
                } else {
                    let new_min = local - drag.grab;
                    panel.rect = egui::Rect::from_min_size(new_min, panel.rect.size());
                }
            }
        }

        // Draw panels back-to-front.
        for (i, panel) in self.layout_panels.iter().enumerate() {
            let r = panel.rect.translate(origin.to_vec2());
            let selected = self.layout_selected == Some(i);
            painter.rect_filled(r, 2.0_f32, egui::Color32::WHITE);

            // Title (falls back to the default title for this panel when blank).
            let title_text = if panel.title.trim().is_empty() {
                self.default_panel_title(panel)
            } else {
                panel.title.clone()
            };
            painter.text(
                r.min + egui::vec2(r.width() / 2.0, -6.0),
                egui::Align2::CENTER_BOTTOM,
                &title_text,
                egui::FontId::proportional(panel.title_size),
                egui::Color32::from_gray(60),
            );

            // Preview points (histogram panels render as bars, not dots).
            let inner = r.shrink(8.0);
            let dot = egui::Color32::from_rgba_unmultiplied(
                self.point_color.r(),
                self.point_color.g(),
                self.point_color.b(),
                180,
            );
            if panel.view == ViewMode::Histogram {
                draw_preview_histogram(&painter, inner, &panel.preview, dot);
            } else {
                for pt in &panel.preview {
                    let x = inner.min.x + pt[0].clamp(0.0, 1.0) * inner.width();
                    let y = inner.max.y - pt[1].clamp(0.0, 1.0) * inner.height();
                    painter.rect_filled(
                        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(1.5, 1.5)),
                        0.0_f32,
                        dot,
                    );
                }
            }

            // Axis labels (using the per-panel label style).
            let fcs = &self.samples[panel.sample].fcs;
            let x_text = panel
                .x_label
                .text
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| param_display_name(fcs, panel.x));
            let y_text = panel
                .y_label
                .text
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| {
                    if matches!(panel.view, ViewMode::Histogram) {
                        "Count".to_string()
                    } else {
                        param_display_name(fcs, panel.y)
                    }
                });
            // Panels are authored at 500x450, so scale label text to the panel.
            let lscale = (r.width() / 500.0).clamp(0.25, 4.0);
            paint_label(
                &painter,
                r.min + egui::vec2(r.width() / 2.0, r.height() - 5.0),
                &x_text,
                panel.x_label.size * lscale,
                panel.x_label.color,
                panel.x_label.font.egui(),
                panel.x_label.rotation_deg.to_radians(),
            );
            paint_label(
                &painter,
                r.min + egui::vec2(6.0, r.height() / 2.0),
                &y_text,
                panel.y_label.size * lscale,
                panel.y_label.color,
                panel.y_label.font.egui(),
                panel.y_label.rotation_deg.to_radians(),
            );

            let stroke = if selected {
                egui::Stroke::new(2.0_f32, p.teal)
            } else {
                egui::Stroke::new(1.0_f32, p.border)
            };
            painter.rect_stroke(r, 2.0_f32, stroke);

            if selected {
                let handle = egui::Rect::from_min_size(
                    r.max - egui::vec2(14.0, 14.0),
                    egui::vec2(14.0, 14.0),
                );
                painter.rect_filled(handle, 2.0_f32, p.teal);
            }
        }
    }

    /// Draw one detached graph window with its own plot spec (V7).
    fn popout_window(&mut self, ctx: &egui::Context, idx: usize, st: &mut PopoutState) {
        let p = self.palette();
        let mut open = st.open;
        let gate_idx = st.gate;
        let title = gate_idx
            .and_then(|i| self.gates.get(i))
            .map(|g| format!("Population: {}", g.name))
            .unwrap_or_else(|| "Population".to_string());
        let response = egui::Window::new(title)
            .id(egui::Id::new(("popout_window", idx)))
            .open(&mut open)
            .resizable(true)
            .default_size([760.0, 600.0])
            .show(ctx, |ui| {
                card_frame(p).show(ui, |ui| {
                    let Some(i) = gate_idx else {
                        ui.label(
                            egui::RichText::new("Select a gate in the tree.")
                                .color(p.muted)
                                .small(),
                        );
                        return;
                    };
                    if self.current_sample.is_none() {
                        ui.label(
                            egui::RichText::new("No sample selected.")
                                .color(p.muted)
                                .small(),
                        );
                        return;
                    }

                    ui.horizontal(|ui| {
                        if primary_button(ui, p, "Add this plot to layout")
                            .on_hover_text("Add this detached population view to the Layout Editor")
                            .clicked()
                        {
                            self.add_layout_popout(st);
                        }
                    });
                    ui.separator();

                    // The detached window exposes the same controls as the main
                    // window: axes, ranges, scales, colour, view and gating.
                    self.popout_controls(ui, p, i, st, idx);
                    ui.separator();

                    let (Some(x), Some(y)) = (st.x_param, st.y_param) else {
                        ui.label(
                            egui::RichText::new("Select X and Y parameters.")
                                .color(p.muted)
                                .small(),
                        );
                        return;
                    };
                    let (x_label, y_label) = match self.current_fcs() {
                        Some(f) => (param_display_name(f, x), param_display_name(f, y)),
                        None => (String::new(), String::new()),
                    };
                    let pts = self.population_points_xy(i, x, y);
                    let visuals =
                        self.gate_visuals_for(x, y, st.x_range, st.y_range, st.selected_gate);
                    let handles = match st.selected_gate {
                        Some(gi)
                            if self.gates.get(gi).map(|g| (g.x_idx, g.y_idx)) == Some((x, y)) =>
                        {
                            self.gates[gi].kind.handles()
                        }
                        _ => Vec::new(),
                    };
                    let cfg = PlotCfg {
                        points: pts,
                        x_range: st.x_range,
                        y_range: st.y_range,
                        x_scale: st.x_scale.clone(),
                        y_scale: st.y_scale.clone(),
                        point_color: st.point_color,
                        gradient: st.gradient,
                        view: st.view,
                        density_map: st.density_map,
                        data_overlays: self.data_overlays_for_plot(),
                        contour_levels: st.contour_levels,
                        gates: visuals,
                        handles,
                        x_label: format!("{x_label} ({})", scale_label(&st.x_scale)),
                        y_label: if matches!(st.view, ViewMode::Histogram) {
                            "Count".to_string()
                        } else {
                            format!("{y_label} ({})", scale_label(&st.y_scale))
                        },
                        interactive: true,
                        line_preview: matches!(st.new_gate_type, NewGateType::Bilinear),
                        smoothing: self.plot_smoothing,
                        resolution: self.plot_resolution,
                        outlier_clip_percent: self.outlier_clip_percent,
                        large_dots: self.large_dots,
                        grid: self.show_grid,
                        medians: self.show_medians,
                        show_event_numbers: self.show_event_numbers,
                        show_names_frequencies: self.show_names_frequencies,
                        notation: self.tick_notation,
                        show_axis_labels: self.show_axis_labels,
                        show_ticks: self.show_ticks,
                        show_numbers: self.show_numbers,
                        axis_histograms: self.show_axis_histograms,
                        transposed: false,
                    };
                    let mut cfg = cfg;
                    self.apply_transpose_to_cfg(&mut cfg);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} events shown — drag to gate, drag handles to edit",
                            cfg.points.len()
                        ))
                        .small()
                        .color(p.muted),
                    );
                    if matches!(st.view, ViewMode::Pseudocolor | ViewMode::Zebra) {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("density").small().color(p.muted));
                            ui.label(egui::RichText::new("low").small().color(p.muted));
                            density_legend_ui(ui, &st.density_map);
                            ui.label(egui::RichText::new("high").small().color(p.muted));
                        });
                    }

                    let mut inter = PlotInteraction {
                        drag_start: st.interact.drag_start,
                        drag_current: st.interact.drag_current,
                        ..Default::default()
                    };
                    show_plot(ui, &format!("popout_plot_{idx}"), &cfg, &mut inter);

                    let tol = 0.02
                        * (st.x_range.1 - st.x_range.0)
                            .abs()
                            .max((st.y_range.1 - st.y_range.0).abs())
                            .max(1.0);
                    let outcome = drive_interact(
                        &mut st.interact,
                        &inter,
                        &self.gates,
                        st.selected_gate,
                        st.new_gate_type,
                        tol,
                    );
                    if let Some(outcome) = outcome {
                        match outcome {
                            InteractOutcome::Create { start, end } => {
                                let span = (st.x_range.1 - st.x_range.0)
                                    .abs()
                                    .max((st.y_range.1 - st.y_range.0).abs())
                                    .max(1e-9);
                                let created = self.create_gates(
                                    start,
                                    end,
                                    x,
                                    y,
                                    st.gate,
                                    st.new_gate_type,
                                    span,
                                );
                                if let Some(&last) = created.last() {
                                    st.selected_gate = Some(last);
                                }
                            }
                            InteractOutcome::Update { gate, kind } => {
                                st.selected_gate = Some(gate);
                                let old = self.gates.get(gate).map(|g| g.kind.clone());
                                if gate < self.gates.len() {
                                    self.gates[gate].kind = kind;
                                }
                                if let Some(old) = old {
                                    let new_kind = self.gates.get(gate).map(|g| g.kind.clone());
                                    if let Some(new_kind) = new_kind {
                                        self.commit_gate_edit(gate, &old, &new_kind);
                                    }
                                }
                                self.recompute_populations();
                            }
                            InteractOutcome::Select(gate) => {
                                st.selected_gate = Some(gate);
                            }
                            InteractOutcome::Click(p) => self.create_click_gate(p, Some(idx)),
                        }
                    }
                });
            });

        if let Some(resp) = response
            && resp.response.hovered()
        {
            self.active_popout = Some(idx);
        }
        st.open = open;
    }

    /// The floating layout-editor window.
    fn layout_window(&mut self, ctx: &egui::Context) {
        if !self.layout_open {
            return;
        }
        let p = self.palette();
        let mut open = self.layout_open;
        egui::Window::new("Layout Editor")
            .open(&mut open)
            .resizable(true)
            .default_size([980.0, 700.0])
            .show(ctx, |ui| {
                card_frame(p).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if primary_button(ui, p, "Add current plot").clicked() {
                            self.add_layout_current();
                        }
                        let popout_open = self.popouts.iter().any(|st| st.open);
                        if ui
                            .add_enabled(
                                popout_open,
                                egui::Button::new(
                                    egui::RichText::new("Add detached plot")
                                        .small()
                                        .color(p.ink),
                                )
                                .fill(p.soft)
                                .stroke(egui::Stroke::new(1.0_f32, p.border))
                                .rounding(6.0),
                            )
                            .clicked()
                        {
                            self.add_layout_popout_active();
                        }
                        if secondary_button(ui, p, "Tile").clicked() {
                            self.tile_layout();
                        }
                        ui.label(egui::RichText::new("Columns:").small().color(p.muted));
                        ui.add(
                            egui::DragValue::new(&mut self.layout_columns).range(1..=4),
                        );
                        if secondary_button(ui, p, "Bring to front").clicked()
                            && let Some(i) = self.layout_selected
                                && i < self.layout_panels.len() {
                                    let p = self.layout_panels.remove(i);
                                    self.layout_panels.push(p);
                                    self.layout_selected = Some(self.layout_panels.len() - 1);
                                }
                        if secondary_button(ui, p, "Delete").clicked()
                            && let Some(i) = self.layout_selected {
                                if i < self.layout_panels.len() {
                                    self.layout_panels.remove(i);
                                }
                                self.layout_selected = None;
                            }
                        if secondary_button(ui, p, "Clear").clicked() {
                            self.layout_panels.clear();
                            self.layout_selected = None;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Export:").small().color(p.muted));
                        if secondary_button(ui, p, "SVG…").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("SVG", &["svg"])
                                .set_file_name("layout.svg")
                                .save_file()
                            {
                                self.export_layout(&path);
                            }
                        if secondary_button(ui, p, "Image…").clicked() {
                            let mut dlg = rfd::FileDialog::new().set_file_name("layout.png");
                            for &(name, exts) in ImageFormat::dialog_filters() {
                                dlg = dlg.add_filter(name, exts);
                            }
                            if let Some(path) = dlg.save_file() {
                                self.export_layout_image(&path);
                            }
                        }
                        ui.label(egui::RichText::new("scale:").small().color(p.muted));
                        ui.add(
                            egui::DragValue::new(&mut self.export_scale)
                                .range(1.0..=4.0)
                                .speed(0.1),
                        );
                        ui.label(
                            egui::RichText::new(
                                "Tip: drag a panel to move it; drag its corner handle to resize.",
                            )
                            .small()
                            .color(p.muted),
                        );
                    });
                    ui.separator();

                    egui::ScrollArea::both().show(ui, |ui| {
                        self.layout_canvas(ui, p);
                    });

                    // Per-axis label styling for the selected panel.
                    if let Some(i) = self.layout_selected
                        && let Some(panel) = self.layout_panels.get_mut(i) {
                            ui.separator();
                            ui.label(
                                egui::RichText::new("Axis label style (selected panel)")
                                    .strong()
                                    .color(p.ink),
                            );
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("X text:").small().color(p.muted));
                                label_text_edit(ui, "lx_text", &mut panel.x_label);
                                ui.label(egui::RichText::new("size:").small().color(p.muted));
                                ui.add(
                                    egui::DragValue::new(&mut panel.x_label.size)
                                        .range(4.0..=72.0)
                                        .speed(0.5),
                                );
                                ui.label(egui::RichText::new("rotate:").small().color(p.muted));
                                ui.add(
                                    egui::DragValue::new(&mut panel.x_label.rotation_deg)
                                        .range(-180.0..=180.0)
                                        .suffix("°"),
                                );
                                label_font_combo(ui, "lx_font", &mut panel.x_label.font);
                                ui.color_edit_button_srgba(&mut panel.x_label.color);
                            });
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Y text:").small().color(p.muted));
                                label_text_edit(ui, "ly_text", &mut panel.y_label);
                                ui.label(egui::RichText::new("size:").small().color(p.muted));
                                ui.add(
                                    egui::DragValue::new(&mut panel.y_label.size)
                                        .range(4.0..=72.0)
                                        .speed(0.5),
                                );
                                ui.label(egui::RichText::new("rotate:").small().color(p.muted));
                                ui.add(
                                    egui::DragValue::new(&mut panel.y_label.rotation_deg)
                                        .range(-180.0..=180.0)
                                        .suffix("°"),
                                );
                                label_font_combo(ui, "ly_font", &mut panel.y_label.font);
                                ui.color_edit_button_srgba(&mut panel.y_label.color);
                            });
                            ui.weak(
                                egui::RichText::new(
                                    "Leave a text box blank to use the parameter name. The (linear)/(log) suffix is kept.",
                                )
                                .color(p.muted),
                            );

                            ui.separator();
                            ui.label(
                                egui::RichText::new("Plot title style (selected panel)")
                                    .strong()
                                    .color(p.ink),
                            );
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Title text:").small().color(p.muted));
                                ui.add(
                                    egui::TextEdit::singleline(&mut panel.title)
                                        .id_salt("layout_panel_title")
                                        .hint_text("Plot title")
                                        .desired_width(140.0),
                                );
                                ui.label(egui::RichText::new("size:").small().color(p.muted));
                                ui.add(egui::Slider::new(&mut panel.title_size, 4.0..=72.0));
                            });
                            ui.weak(
                                egui::RichText::new(
                                    "Leave the title text blank to use the default title.",
                                )
                                .color(p.muted),
                            );
                        }

                    // Per-panel quick list (select / remove).
                    ui.separator();
                    let mut remove: Option<usize> = None;
                    for (i, panel) in self.layout_panels.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let name = self
                                .samples
                                .get(panel.sample)
                                .map(|s| s.id.clone())
                                .unwrap_or_else(|| "?".into());
                            let label = format!(
                                "{}. {} — {} vs {} ({})",
                                i + 1,
                                name,
                                param_display_name(&self.samples[panel.sample].fcs, panel.x),
                                param_display_name(&self.samples[panel.sample].fcs, panel.y),
                                panel.view.label()
                            );
                            if ui
                                .selectable_label(self.layout_selected == Some(i), label)
                                .clicked()
                            {
                                self.layout_selected = Some(i);
                            }
                            if secondary_button(ui, p, "✖").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        self.layout_panels.remove(i);
                        if self.layout_selected == Some(i) {
                            self.layout_selected = None;
                        }
                    }
                });
            });
        self.layout_open = open;
    }
}

/// The pixel size of the layout canvas implied by the placed panels.
fn layout_canvas_size(panels: &[LayoutPanelSpec]) -> (u32, u32) {
    // Content sits below a title strip inside each panel, so reserve room for
    // it or the bottom axis labels get clipped in raster exports.
    let title_h = LayoutOptions::default().title_height as f32;
    let mut max_x = 100.0f32;
    let mut max_y = 100.0f32;
    for p in panels {
        max_x = max_x.max(p.rect.max.x);
        max_y = max_y.max(p.rect.max.y + title_h);
    }
    ((max_x + 20.0) as u32, (max_y + 20.0) as u32)
}

fn scale_label(s: &AxisScale) -> &'static str {
    match s {
        AxisScale::Linear => "linear",
        AxisScale::Log => "log10",
        AxisScale::Arcsinh { .. } => "arcsinh",
        AxisScale::Biexponential(_) => "biexponential",
        AxisScale::Logicle(_) => "logicle",
        AxisScale::Hyperlog(_) => "hyperlog",
        AxisScale::RobustLog { .. } => "robust log",
        AxisScale::Miltenyi { .. } => "miltenyi",
        AxisScale::Lookup { .. } => "lookup",
    }
}

fn quad_label(q: &Quadrant) -> &'static str {
    match q {
        Quadrant::UpperLeft => "UL",
        Quadrant::UpperRight => "UR",
        Quadrant::LowerLeft => "LL",
        Quadrant::LowerRight => "LR",
    }
}

/// Actions produced while drawing the recursive gate tree.
enum TreeAction {
    Select(usize),
    Remove(usize),
    Popout(usize),
}

/// Renders the hierarchical gate tree, returning actions for the caller.
fn gate_tree_ui(
    ui: &mut egui::Ui,
    gates: &[GateDef],
    children: &[Vec<usize>],
    counts: &[usize],
    total: usize,
    selected: Option<usize>,
) -> Vec<TreeAction> {
    let mut actions = Vec::new();
    let roots: Vec<usize> = (0..gates.len())
        .filter(|i| gates[*i].parent.is_none())
        .collect();

    #[allow(clippy::too_many_arguments)]
    fn rec(
        node: usize,
        depth: usize,
        ui: &mut egui::Ui,
        gates: &[GateDef],
        children: &[Vec<usize>],
        counts: &[usize],
        total: usize,
        selected: Option<usize>,
        actions: &mut Vec<TreeAction>,
    ) {
        let gate = &gates[node];
        let count = counts.get(node).copied().unwrap_or(0);
        let parent_count = gate.parent.and_then(|p| counts.get(p).copied());
        // A nested gate gets both denominators: its parent population and the
        // whole sample. A root gate has no parent, so only show the total.
        let pct_label = gate_percentage_label(count, parent_count, total);
        // Gates created later in the list are drawn on top of earlier ones, so
        // a gate with a later sibling is overlapped and cannot be picked by
        // clicking the plot until the user selects it explicitly.
        let covered = children[node].iter().any(|&c| c > node);

        ui.horizontal(|ui| {
            ui.add_space((depth as f32) * 12.0);
            let swatch = egui::Frame::none()
                .fill(gate.color)
                .rounding(2.0)
                .inner_margin(egui::Margin::same(3.0));
            swatch.show(ui, |ui| {
                ui.label(" ");
            });
            let mut label = format!(
                "{}  [{}]  {} ({})",
                gate.name,
                gate.kind.label(),
                count,
                pct_label
            );
            if covered {
                label.push_str("  [overlapped]");
            }
            if ui.selectable_label(selected == Some(node), label).clicked() {
                actions.push(TreeAction::Select(node));
            }
            if ui
                .small_button("◧")
                .on_hover_text("Show in detached window")
                .clicked()
            {
                actions.push(TreeAction::Popout(node));
            }
            if ui.small_button("✖").on_hover_text("Delete gate").clicked() {
                actions.push(TreeAction::Remove(node));
            }
        });

        for &child in &children[node] {
            rec(
                child,
                depth + 1,
                ui,
                gates,
                children,
                counts,
                total,
                selected,
                actions,
            );
        }
    }

    if roots.is_empty() {
        ui.weak("No gates yet — draw one on the plot.");
    }
    for root in roots {
        rec(
            root,
            0,
            ui,
            gates,
            children,
            counts,
            total,
            selected,
            &mut actions,
        );
    }
    actions
}

impl FlowersApp {
    /// Intercepts the OS window's close button and routes it through the
    /// unsaved-changes confirmation. The close is canceled for this frame;
    /// the dialog then either saves, closes explicitly, or lets the user
    /// cancel. `allow_close` lets an explicit "quit without saving" through.
    fn handle_window_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.dirty && !self.allow_close {
            self.quit_confirm = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }

    /// Draw the whole application UI for one frame.
    ///
    /// Kept separate from [`eframe::App::update`] so tests can drive the real
    /// layout through an [`egui::Context`] without an `eframe::Frame`.
    fn draw(&mut self, ctx: &egui::Context) {
        // Apply the selected theme before drawing anything this frame.
        ctx.set_visuals(match self.theme {
            Theme::Dark => egui::Visuals::dark(),
            Theme::Light => egui::Visuals::light(),
        });

        // The window's X button must go through the same unsaved-changes
        // prompt as File ▸ Quit.
        self.handle_window_close_request(ctx);

        // ----- 3D view (V3) -----
        if self.view3d_open {
            let mut open = self.view3d_open;
            egui::Window::new("3D View")
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(660.0)
                .show(ctx, |ui| {
                    let names: Vec<String> = self
                        .current_fcs()
                        .map(|fcs| {
                            (0..fcs.parameters.len())
                                .map(|i| param_display_name(fcs, i))
                                .collect()
                        })
                        .unwrap_or_default();

                    ui.horizontal(|ui| {
                        ui.label("Mode:");
                        egui::ComboBox::new("view3d_mode", "")
                            .selected_text(self.view3d_spec.mode.label())
                            .show_ui(ui, |ui| {
                                for mode in crate::plot3d::View3DMode::all() {
                                    ui.selectable_value(
                                        &mut self.view3d_spec.mode,
                                        mode,
                                        mode.label(),
                                    );
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label("X:");
                        egui::ComboBox::new("view3d_x", "")
                            .selected_text(
                                names.get(self.view3d_spec.x).cloned().unwrap_or_default(),
                            )
                            .show_ui(ui, |ui| {
                                for (i, name) in names.iter().enumerate() {
                                    ui.selectable_value(&mut self.view3d_spec.x, i, name);
                                }
                            });
                        ui.label("Y:");
                        egui::ComboBox::new("view3d_y", "")
                            .selected_text(
                                names.get(self.view3d_spec.y).cloned().unwrap_or_default(),
                            )
                            .show_ui(ui, |ui| {
                                for (i, name) in names.iter().enumerate() {
                                    ui.selectable_value(&mut self.view3d_spec.y, i, name);
                                }
                            });
                        ui.label("Z:");
                        egui::ComboBox::new("view3d_z", "")
                            .selected_text(
                                names.get(self.view3d_spec.z).cloned().unwrap_or_default(),
                            )
                            .show_ui(ui, |ui| {
                                for (i, name) in names.iter().enumerate() {
                                    ui.selectable_value(&mut self.view3d_spec.z, i, name);
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Colour by:");
                        egui::ComboBox::new("view3d_color", "")
                            .selected_text(
                                self.view3d_spec
                                    .color
                                    .and_then(|i| names.get(i).cloned())
                                    .unwrap_or_else(|| "Z".to_string()),
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.view3d_spec.color, None, "Z");
                                for (i, name) in names.iter().enumerate() {
                                    ui.selectable_value(&mut self.view3d_spec.color, Some(i), name);
                                }
                            });
                        ui.label("Size by:");
                        egui::ComboBox::new("view3d_size", "")
                            .selected_text(
                                self.view3d_spec
                                    .size
                                    .and_then(|i| names.get(i).cloned())
                                    .unwrap_or_else(|| "uniform".to_string()),
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.view3d_spec.size, None, "uniform");
                                for (i, name) in names.iter().enumerate() {
                                    ui.selectable_value(&mut self.view3d_spec.size, Some(i), name);
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Slider::new(&mut self.view3d_spec.camera.yaw_deg, -180.0..=180.0)
                                .text("yaw°"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.view3d_spec.camera.pitch_deg, -89.0..=89.0)
                                .text("pitch°"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.view3d_spec.camera.zoom, 0.2..=3.0)
                                .text("zoom"),
                        );
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Export SVG").clicked() {
                            self.export_view3d_svg();
                        }
                        if ui.button("Export animation…").clicked() {
                            self.export_view3d_animation();
                        }
                        ui.add(
                            egui::DragValue::new(&mut self.view3d_frames)
                                .range(2..=240)
                                .prefix("frames: "),
                        );
                        if ui.button("Close").clicked() {
                            self.view3d_open = false;
                        }
                    });

                    // Interactive preview drawn with the egui painter.
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(620.0, 440.0), egui::Sense::hover());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(248, 250, 252));
                    let camera = self.view3d_spec.camera;
                    let w = rect.width() as u32;
                    let h = rect.height() as u32;
                    // Cube frame.
                    let corners = [
                        [-1.0, -1.0, -1.0],
                        [1.0, -1.0, -1.0],
                        [1.0, 1.0, -1.0],
                        [-1.0, 1.0, -1.0],
                        [-1.0, -1.0, 1.0],
                        [1.0, -1.0, 1.0],
                        [1.0, 1.0, 1.0],
                        [-1.0, 1.0, 1.0],
                    ];
                    let proj: Vec<[f64; 2]> = corners
                        .iter()
                        .map(|&c| crate::plot3d::project_point(c, &camera, w, h).screen)
                        .collect();
                    let edges = [
                        (0, 1),
                        (1, 2),
                        (2, 3),
                        (3, 0),
                        (4, 5),
                        (5, 6),
                        (6, 7),
                        (7, 4),
                        (0, 4),
                        (1, 5),
                        (2, 6),
                        (3, 7),
                    ];
                    let stroke = egui::Stroke::new(0.8_f32, egui::Color32::from_gray(160));
                    for (a, b) in edges {
                        let pa = egui::pos2(
                            rect.min.x + proj[a][0] as f32,
                            rect.min.y + proj[a][1] as f32,
                        );
                        let pb = egui::pos2(
                            rect.min.x + proj[b][0] as f32,
                            rect.min.y + proj[b][1] as f32,
                        );
                        painter.line_segment([pa, pb], stroke);
                    }
                    // Depth-sorted shapes.
                    let shapes = self
                        .current_fcs()
                        .map(|fcs| {
                            crate::plot3d::preview_shapes(&fcs.data, &self.view3d_spec, w, h)
                        })
                        .unwrap_or_default();
                    for shape in shapes {
                        match shape {
                            crate::plot3d::PreviewShape::Dot {
                                center,
                                radius,
                                color,
                                opacity,
                            } => {
                                painter.circle_filled(
                                    egui::pos2(
                                        rect.min.x + center[0] as f32,
                                        rect.min.y + center[1] as f32,
                                    ),
                                    radius as f32,
                                    egui::Color32::from_rgba_unmultiplied(
                                        color.0,
                                        color.1,
                                        color.2,
                                        (opacity * 255.0).round() as u8,
                                    ),
                                );
                            }
                            crate::plot3d::PreviewShape::Poly { points, color } => {
                                let pts: Vec<egui::Pos2> = points
                                    .iter()
                                    .map(|p| {
                                        egui::pos2(
                                            rect.min.x + p[0] as f32,
                                            rect.min.y + p[1] as f32,
                                        )
                                    })
                                    .collect();
                                painter.add(egui::Shape::convex_polygon(
                                    pts,
                                    egui::Color32::from_rgb(color.0, color.1, color.2),
                                    egui::Stroke::new(0.4_f32, egui::Color32::from_gray(90)),
                                ));
                            }
                        }
                    }
                });
        }

        // ----- Statistics panel (S5) -----
        // Window listing the statistic sets configured for the current analysis,
        // with add / copy / remove / export actions and per-set statistics.
        if self.stats_panel_open {
            egui::Window::new("Statistics")
                .collapsible(false)
                .resizable(true)
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.heading("Statistics");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Add statistic set").clicked() {
                            let count = self.statistic_sets.len();
                            if count > 0 {
                                self.stats_panel_add_set(count - 1);
                            }
                        }
                        if ui.button("Copy set").clicked() {
                            let count = self.statistic_sets.len();
                            if count > 0 {
                                self.stats_panel_copy_set(count - 1);
                            }
                        }
                        if ui.button("Remove set").clicked() {
                            let count = self.statistic_sets.len();
                            if count > 0 {
                                self.stats_panel_remove_set(count - 1);
                            }
                        }
                        if ui.button("Export sets").clicked()
                            && let Err(message) = self.stats_panel_export()
                        {
                            self.set_status(format!("Export failed: {message}"));
                        }
                        if ui.button("Export statistics CSV").clicked() {
                            let sample_name = self
                                .current_sample
                                .map(|i| self.samples[i].id.clone())
                                .unwrap_or_else(|| "sample".to_string());
                            let profiles = self.stats_panel_profiles();
                            let rows = statistics_rows(&sample_name, &profiles);
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("CSV", &["csv"])
                                .set_file_name("statistics.csv")
                                .save_file()
                            {
                                if let Err(e) = write_statistics_csv(&path, &rows) {
                                    self.set_status(format!("Export failed: {e}"));
                                } else {
                                    self.set_status(format!(
                                        "Exported {} statistics rows to {}",
                                        rows.len(),
                                        path.display()
                                    ));
                                }
                            }
                        }
                        if ui.button("Close").clicked() {
                            self.stats_panel_open = false;
                        }
                    });
                    ui.add_space(6.0);
                    if self.statistic_sets.is_empty() {
                        ui.weak("No statistic sets yet. Add one to start computing.");
                    }
                    for i in 0..self.statistic_sets.len() {
                        let set_name = self.statistic_sets[i].name.clone();
                        let stat_count = self.statistic_sets[i].stats.len();
                        ui.collapsing(set_name, |ui| {
                            let rows = self.stats_panel_rows();
                            for row in rows {
                                ui.label(format!(
                                    "  {} / {} = {:.4}",
                                    row.parameter,
                                    crate::statistics::statistic_label(&row.statistic),
                                    row.value
                                ));
                            }
                            ui.weak(format!("{stat_count} statistic(s) configured"));
                        });
                        if ui.small_button("✖").clicked() {
                            self.stats_panel_remove_set(i);
                        }
                    }
                });
        }

        // ----- Table editor window -----
        // Visible window that lists the computed statistics as a table and
        // exports them, so users have an explicit way to see / export the
        // per-population data instead of hunting through plots.
        if self.table_open {
            egui::Window::new("Table Editor")
                .collapsible(false)
                .resizable(true)
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.heading("Table Editor");
                        ui.add_space(4.0);
                        if self.statistic_sets.is_empty() {
                            ui.weak(
                                "No statistic sets yet. Add one from the Statistics panel first.",
                            );
                        }
                        ui.add_space(4.0);
                        let stats = self.stats_panel_rows();
                        for row in stats {
                            ui.label(format!(
                                "{} / {} = {}",
                                row.parameter,
                                crate::statistics::statistic_label(&row.statistic),
                                row.value
                            ));
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Export statistics CSV").clicked() {
                                let sample_name = self
                                    .current_sample
                                    .map(|i| self.samples[i].id.clone())
                                    .unwrap_or_else(|| "sample".to_string());
                                let profiles = self.stats_panel_profiles();
                                let rows = statistics_rows(&sample_name, &profiles);
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("CSV", &["csv"])
                                    .set_file_name("statistics.csv")
                                    .save_file()
                                {
                                    if let Err(e) = write_statistics_csv(&path, &rows) {
                                        self.set_status(format!("Export failed: {e}"));
                                    } else {
                                        self.set_status(format!(
                                            "Exported {} statistics rows to {}",
                                            rows.len(),
                                            path.display()
                                        ));
                                    }
                                }
                            }
                            if ui.button("Close").clicked() {
                                self.table_open = false;
                            }
                        });
                    });
                });
        }

        // ----- Quit confirmation -----
        // Shown when the user asks to quit with unsaved changes; offers to save
        // first, quit without saving, or cancel.
        if self.quit_confirm {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label("You have unsaved changes.");
                        ui.label("Save your workspace before closing the window?");
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Save Workspace…").clicked()
                                && let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Flowers workspace", &["wsp", "json"])
                                    .set_file_name("workspace.wsp")
                                    .save_file()
                            {
                                self.save_workspace(&path);
                                // Keep the dialog open if saving failed so
                                // the user can retry or quit explicitly.
                                if !self.dirty {
                                    self.quit_confirm = false;
                                    self.allow_close = true;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            }
                            if ui.button("Quit Without Saving").clicked() {
                                self.quit_confirm = false;
                                self.allow_close = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            if ui.button("Cancel").clicked() {
                                self.quit_confirm = false;
                            }
                        });
                    });
                });
        }

        // ----- Undo / redo keyboard shortcuts -----
        // Skipped while a text field has focus so the widget keeps its own undo.
        if !ctx.wants_keyboard_input() {
            let cmd = egui::Modifiers::COMMAND;
            // Most specific shortcut first: extra Shift is ignored by matching.
            let redo_shift =
                ctx.input_mut(|i| i.consume_key(cmd | egui::Modifiers::SHIFT, egui::Key::Z));
            let redo_y = ctx.input_mut(|i| i.consume_key(cmd, egui::Key::Y));
            let undo = ctx.input_mut(|i| i.consume_key(cmd, egui::Key::Z));
            if redo_shift || redo_y {
                self.redo();
            } else if undo {
                self.undo();
            }

            // Gate clipboard shortcuts. Text editors keep their own Ctrl+C/V
            // because this block only runs when no widget wants keyboard input.
            let copy = ctx.input_mut(|i| i.consume_key(cmd, egui::Key::C));
            let paste = ctx.input_mut(|i| i.consume_key(cmd, egui::Key::V));
            let duplicate = ctx.input_mut(|i| i.consume_key(cmd, egui::Key::D));
            if copy && let Some(i) = self.selected_gate {
                if let Some(json) = self.copy_gate(i) {
                    ctx.copy_text(json);
                }
            } else if paste {
                self.paste_gate();
            } else if duplicate && let Some(i) = self.selected_gate {
                self.duplicate_gate(i);
            }

            // Graph-window navigation shortcuts (V7). Text fields keep their
            // own undo stack, so these only run while no widget wants keys.
            let alt = egui::Modifiers::ALT;
            let prev_sample = ctx.input_mut(|i| i.consume_key(alt, egui::Key::PageUp));
            let next_sample = ctx.input_mut(|i| i.consume_key(alt, egui::Key::PageDown));
            if prev_sample {
                self.step_sample(-1);
            } else if next_sample {
                self.step_sample(1);
            }

            let open_bracket = ctx.input_mut(|i| i.consume_key(cmd, egui::Key::OpenBracket));
            let close_bracket = ctx.input_mut(|i| i.consume_key(cmd, egui::Key::CloseBracket));
            let next_sibling = ctx.input_mut(|i| {
                i.consume_key(cmd | egui::Modifiers::SHIFT, egui::Key::CloseBracket)
            });
            let dup_window =
                ctx.input_mut(|i| i.consume_key(cmd | egui::Modifiers::SHIFT, egui::Key::D));
            if let Some(nav) = if open_bracket {
                Some(PopNav::Parent)
            } else if close_bracket {
                Some(PopNav::FirstChild)
            } else if next_sibling {
                Some(PopNav::NextSibling)
            } else {
                None
            } {
                let idx = self
                    .active_popout
                    .filter(|&i| self.popouts.get(i).is_some_and(|s| s.open));
                if let Some(i) = idx {
                    let mut st = std::mem::take(&mut self.popouts[i]);
                    self.navigate_popout(&mut st, nav);
                    self.popouts[i] = st;
                }
            }
            if dup_window {
                self.duplicate_active_popout();
            }

            let prev_x = ctx.input_mut(|i| i.consume_key(alt, egui::Key::ArrowLeft));
            let next_x = ctx.input_mut(|i| i.consume_key(alt, egui::Key::ArrowRight));
            let prev_y = ctx.input_mut(|i| i.consume_key(alt, egui::Key::ArrowUp));
            let next_y = ctx.input_mut(|i| i.consume_key(alt, egui::Key::ArrowDown));
            let x_delta = if prev_x {
                -1
            } else if next_x {
                1
            } else {
                0
            };
            let y_delta = if prev_y {
                -1
            } else if next_y {
                1
            } else {
                0
            };
            if x_delta != 0 {
                self.cycle_plot_param(true, x_delta as isize);
            }
            if y_delta != 0 {
                self.cycle_plot_param(false, y_delta as isize);
            }
        }

        // ----- Top header: brand + menu bar -----
        let p = self.palette();
        egui::TopBottomPanel::top("header")
            .frame(panel_frame(p))
            .show(ctx, |ui| {
                header_bar(ui, p, |ui| {
                    // Standalone top-bar tools (moved out of the File menu).
                    if ui.button("Statistics Panel…").clicked() {
                        self.open_stats_panel();
                    }
                    if ui.button("Table Editor…").clicked() {
                        self.table_open = true;
                    }
                    // File dropdown.
                    ui.menu_button("File", |ui| {
                        if ui.button("Load FCS").clicked() {
                            ui.close_menu();
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                self.load_file(&path);
                            }
                        }
                        ui.separator();
                        if ui.button("New Workspace").clicked() {
                            ui.close_menu();
                            self.new_workspace();
                        }
                        if ui.button("Load Workspace…").clicked() {
                            ui.close_menu();
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Flowers workspace", &["wsp", "json"])
                                .pick_file()
                            {
                                self.load_workspace(&path);
                            }
                        }
                        if ui.button("Save Workspace…").clicked() {
                            ui.close_menu();
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Flowers workspace", &["wsp", "json"])
                                .set_file_name("workspace.wsp")
                                .save_file()
                            {
                                self.save_workspace(&path);
                            }
                        }
                        if ui.button("3D View…").clicked() {
                            ui.close_menu();
                            self.open_view3d();
                        }
                        ui.separator();
                        if ui.button("Import Gating-ML…").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("Gating-ML XML", &["xml"])
                                .pick_file()
                        {
                            ui.close_menu();
                            if let Err(msg) = self.import_gatingml(&path) {
                                self.set_status(msg);
                            }
                        }
                        if ui.button("Export Gating-ML…").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_file_name("gates.gatingml")
                                .add_filter("Gating-ML XML", &["xml"])
                                .save_file()
                            && let Err(msg) = self.export_gatingml(&path)
                        {
                            self.set_status(msg);
                        }
                        if ui.button("Import Mask…").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("Mask", &["txt", "dat"])
                                .pick_file()
                        {
                            ui.close_menu();
                            if let Err(msg) = self.import_mask(&path) {
                                self.set_status(msg);
                            }
                        }
                        ui.separator();
                        if ui.button("Export SVG").clicked() {
                            ui.close_menu();
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("SVG", &["svg"])
                                .set_file_name("plot.svg")
                                .save_file()
                            {
                                self.export_svg(&path);
                            }
                        }
                        if ui.button("Export Image…").clicked() {
                            ui.close_menu();
                            let mut dlg = rfd::FileDialog::new().set_file_name("plot.png");
                            for &(name, exts) in ImageFormat::dialog_filters() {
                                dlg = dlg.add_filter(name, exts);
                            }
                            if let Some(path) = dlg.save_file() {
                                self.export_image(&path);
                            }
                        }
                        if ui.button("Export CSV").clicked() {
                            ui.close_menu();
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("CSV", &["csv"])
                                .set_file_name("populations.csv")
                                .save_file()
                            {
                                self.export_csv(&path);
                            }
                        }
                        ui.menu_button("Multigraphs", |ui| {
                            if ui.button("All parameters by X…").clicked() {
                                ui.close_menu();
                                self.export_multigraph(crate::plot::MultigraphMode::AllByX, 4);
                            }
                            if ui.button("All parameters by Y…").clicked() {
                                ui.close_menu();
                                self.export_multigraph(crate::plot::MultigraphMode::AllByY, 4);
                            }
                            if ui.button("NxN matrix…").clicked() {
                                ui.close_menu();
                                self.export_multigraph(crate::plot::MultigraphMode::NxN, 4);
                            }
                        });
                        ui.separator();
                        if ui.button("Quit").clicked() {
                            ui.close_menu();
                            if self.dirty {
                                self.allow_close = false;
                                self.quit_confirm = true;
                            } else {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                    });

                    // Edit dropdown (undo / redo).
                    ui.menu_button("Edit", |ui| {
                        let undo_shortcut =
                            egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
                        let redo_shortcut = egui::KeyboardShortcut::new(
                            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                            egui::Key::Z,
                        );
                        if ui
                            .add_enabled(
                                self.can_undo(),
                                egui::Button::new("Undo")
                                    .shortcut_text(ui.ctx().format_shortcut(&undo_shortcut)),
                            )
                            .clicked()
                        {
                            ui.close_menu();
                            self.undo();
                        }
                        if ui
                            .add_enabled(
                                self.can_redo(),
                                egui::Button::new("Redo")
                                    .shortcut_text(ui.ctx().format_shortcut(&redo_shortcut)),
                            )
                            .clicked()
                        {
                            ui.close_menu();
                            self.redo();
                        }
                    });

                    // Layout Editor (standalone button).
                    if ui.button("Layout Editor").clicked() {
                        self.layout_open = true;
                    }

                    // Compensation editor.
                    ui.menu_button("Compensation", |ui| {
                        if ui.button("Open compensation editor…").clicked() {
                            ui.close_menu();
                            self.open_compensation();
                        }
                        ui.separator();
                        if ui.button("Remove from current sample").clicked() {
                            ui.close_menu();
                            self.clear_compensation(false);
                        }
                        if ui.button("Remove from all samples").clicked() {
                            ui.close_menu();
                            self.clear_compensation(true);
                        }
                    });

                    // Settings dropdown.
                    ui.menu_button("Settings", |ui| {
                        ui.label("Theme:");
                        for t in [Theme::Dark, Theme::Light] {
                            if ui.selectable_label(self.theme == t, t.label()).clicked() {
                                ui.close_menu();
                                self.theme = t;
                            }
                        }
                    });

                    ui.separator();
                    ui.label("flowers — flow cytometry analysis");
                });
            });

        // ----- Left sidebar: Samples card + Gates card + Controls card -----
        let p = self.palette();
        egui::SidePanel::left("sidebar")
            .default_width(280.0)
            .frame(panel_frame(p))
            .show(ctx, |ui| {
                // The sidebar can grow past the window (many samples / gates /
                // options), so keep it scrollable instead of clipping content.
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // --- Samples card ---
                    card_frame(p).show(ui, |ui| {
                        card_head(ui, p, "Samples", |ui| {
                            ui.label(
                                egui::RichText::new(self.samples.len().to_string())
                                    .small()
                                    .color(p.muted),
                            );
                        });
                        section_label(ui, p, "FCS files in this workspace");
                        ui.add_space(6.0);

                        // Import button.
                        if primary_button(ui, p, "Import FCS").clicked()
                            && let Some(path) = rfd::FileDialog::new().pick_file()
                        {
                            self.load_file(&path);
                        }
                        ui.add_space(8.0);

                        // Sample list.
                        let mut clicked_sample: Option<usize> = None;
                        let mut toggled_gates: Option<(usize, bool)> = None;
                        for (i, sample) in self.samples.iter().enumerate() {
                            let selected = self.current_sample == Some(i);
                            let id = sample.id.clone();
                            let mut apply = sample.apply_gates;
                            let row_fill = if selected { p.teal_light } else { p.soft };
                            let row_stroke = if selected {
                                egui::Stroke::new(1.0_f32, p.teal)
                            } else {
                                egui::Stroke::new(1.0_f32, p.border)
                            };
                            egui::Frame::none()
                                .fill(row_fill)
                                .stroke(row_stroke)
                                .rounding(6.0)
                                .inner_margin(egui::Margin::same(4.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if ui.selectable_label(selected, &id).clicked() {
                                            clicked_sample = Some(i);
                                        }
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .checkbox(&mut apply, "")
                                                    .on_hover_text(
                                                        "Apply the shared gate set to this file",
                                                    )
                                                    .changed()
                                                {
                                                    toggled_gates = Some((i, apply));
                                                }
                                            },
                                        );
                                    });
                                });
                        }
                        if let Some(i) = clicked_sample {
                            self.current_sample = Some(i);
                            self.update_plot_points();
                        }
                        if let Some((i, apply)) = toggled_gates {
                            if let Some(s) = self.samples.get_mut(i) {
                                s.apply_gates = apply;
                            }
                            self.recompute_populations();
                        }
                        if self.samples.is_empty() {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("(none)").color(p.muted).small());
                        }
                        ui.add_space(8.0);
                        section_label(
                            ui,
                            p,
                            "Click a sample to make it active. New gates apply to all samples.",
                        );
                    });

                    ui.add_space(12.0);

                    // --- Gates card ---
                    card_frame(p).show(ui, |ui| {
                        card_head(ui, p, "Gates", |ui| {
                            ui.label(
                                egui::RichText::new(self.gates.len().to_string())
                                    .small()
                                    .color(p.muted),
                            );
                        });
                        section_label(ui, p, "Click a population to work inside it");
                        ui.add_space(6.0);

                        let total = self.current_fcs().map(|f| f.event_count).unwrap_or(0);
                        let counts: Vec<usize> = self.pop_indices.iter().map(|v| v.len()).collect();
                        let actions = gate_tree_ui(
                            ui,
                            &self.gates,
                            &self.children,
                            &counts,
                            total,
                            self.selected_gate,
                        );
                        for action in actions {
                            match action {
                                TreeAction::Select(i) => {
                                    self.selected_gate = Some(i);
                                    self.sync_name_edit();
                                }
                                TreeAction::Remove(i) => self.remove_gate(i),
                                TreeAction::Popout(i) => {
                                    self.open_popout(i);
                                }
                            }
                        }
                        if self.gates.is_empty() {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("No gates yet — draw one on the plot.")
                                    .color(p.muted)
                                    .small(),
                            );
                        }
                    });

                    ui.add_space(12.0);

                    // --- Controls card (plot parameters + gating) ---
                    card_frame(p).show(ui, |ui| {
                        card_head(ui, p, "Controls", |_| {});
                        section_label(ui, p, "Plot parameters & gating");
                        ui.add_space(6.0);

                        let Some(fcs) = self.current_fcs() else {
                            ui.label(
                                egui::RichText::new("No sample selected")
                                    .color(p.muted)
                                    .small(),
                            );
                            return;
                        };
                        let names: Vec<String> = (0..fcs.parameters.len())
                            .map(|i| param_display_name(fcs, i))
                            .collect();
                        let events = fcs.event_count;

                        egui::ComboBox::new("x_param", "X")
                            .selected_text(
                                self.x_param
                                    .map(|i| names[i].clone())
                                    .unwrap_or_else(|| "Select X".to_string()),
                            )
                            .show_ui(ui, |ui| {
                                for (i, name) in names.iter().enumerate() {
                                    if ui
                                        .selectable_value(&mut self.x_param, Some(i), name.clone())
                                        .changed()
                                    {
                                        self.update_plot_points();
                                    }
                                }
                            });
                        egui::ComboBox::new("y_param", "Y")
                            .selected_text(
                                self.y_param
                                    .map(|i| names[i].clone())
                                    .unwrap_or_else(|| "Select Y".to_string()),
                            )
                            .show_ui(ui, |ui| {
                                for (i, name) in names.iter().enumerate() {
                                    if ui
                                        .selectable_value(&mut self.y_param, Some(i), name.clone())
                                        .changed()
                                    {
                                        self.update_plot_points();
                                    }
                                }
                            });

                        if self.x_param.is_some() && self.y_param.is_some() {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!("Events: {events}"))
                                    .small()
                                    .color(p.muted),
                            );
                        }

                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.collapsing("Axis options", |ui| {
                            // Primary scale choices for each axis.
                            ui.horizontal(|ui| {
                                ui.label("X scale:");
                                for scale in AxisScale::primary() {
                                    if ui
                                        .selectable_label(self.x_scale == scale, scale.label())
                                        .clicked()
                                    {
                                        self.x_scale = scale;
                                        self.auto_x();
                                    }
                                }
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label("X range:");
                                let span = (self.x_range.1 - self.x_range.0).abs().max(1.0);
                                ui.add(range_drag_value(&mut self.x_range.0, span));
                                if ui.small_button("−").on_hover_text("Decrease X minimum").clicked() {
                                    adjust_axis_bound(&mut self.x_range, &self.x_scale, true, 0.8);
                                }
                                if ui.small_button("+").on_hover_text("Increase X minimum").clicked() {
                                    adjust_axis_bound(&mut self.x_range, &self.x_scale, true, 1.25);
                                }
                                ui.add(range_drag_value(&mut self.x_range.1, span));
                                if ui.small_button("−").on_hover_text("Decrease X maximum").clicked() {
                                    adjust_axis_bound(&mut self.x_range, &self.x_scale, false, 0.8);
                                }
                                if ui.small_button("+").on_hover_text("Increase X maximum").clicked() {
                                    adjust_axis_bound(&mut self.x_range, &self.x_scale, false, 1.25);
                                }
                                if ui.small_button("−").on_hover_text("Zoom X out").clicked() {
                                    zoom_axis_range(&mut self.x_range, &self.x_scale, 1.25);
                                }
                                if ui.small_button("+").on_hover_text("Zoom X in").clicked() {
                                    zoom_axis_range(&mut self.x_range, &self.x_scale, 0.8);
                                }
                                if ui
                                    .small_button("Auto")
                                    .on_hover_text("Fit X to data")
                                    .clicked()
                                {
                                    self.auto_x();
                                }
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label("Y scale:");
                                for scale in AxisScale::primary() {
                                    if ui
                                        .selectable_label(self.y_scale == scale, scale.label())
                                        .clicked()
                                    {
                                        self.y_scale = scale;
                                        self.auto_y();
                                    }
                                }
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label("Y range:");
                                let span = (self.y_range.1 - self.y_range.0).abs().max(1.0);
                                ui.add(range_drag_value(&mut self.y_range.0, span));
                                if ui.small_button("−").on_hover_text("Decrease Y minimum").clicked() {
                                    adjust_axis_bound(&mut self.y_range, &self.y_scale, true, 0.8);
                                }
                                if ui.small_button("+").on_hover_text("Increase Y minimum").clicked() {
                                    adjust_axis_bound(&mut self.y_range, &self.y_scale, true, 1.25);
                                }
                                ui.add(range_drag_value(&mut self.y_range.1, span));
                                if ui.small_button("−").on_hover_text("Decrease Y maximum").clicked() {
                                    adjust_axis_bound(&mut self.y_range, &self.y_scale, false, 0.8);
                                }
                                if ui.small_button("+").on_hover_text("Increase Y maximum").clicked() {
                                    adjust_axis_bound(&mut self.y_range, &self.y_scale, false, 1.25);
                                }
                                if ui.small_button("−").on_hover_text("Zoom Y out").clicked() {
                                    zoom_axis_range(&mut self.y_range, &self.y_scale, 1.25);
                                }
                                if ui.small_button("+").on_hover_text("Zoom Y in").clicked() {
                                    zoom_axis_range(&mut self.y_range, &self.y_scale, 0.8);
                                }
                                if ui
                                    .small_button("Auto")
                                    .on_hover_text("Fit Y to data")
                                    .clicked()
                                {
                                    self.auto_y();
                                }
                            });
                            ui.add_space(6.0);
                            ui.collapsing("Advanced options", |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("X scale:");
                                    for scale in AxisScale::all() {
                                        if scale.is_linear() || scale == AxisScale::Log {
                                            continue;
                                        }
                                        if ui
                                            .selectable_label(self.x_scale == scale, scale.label())
                                            .clicked()
                                        {
                                            self.x_scale = scale;
                                            self.auto_x();
                                        }
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Y scale:");
                                    for scale in AxisScale::all() {
                                        if scale.is_linear() || scale == AxisScale::Log {
                                            continue;
                                        }
                                        if ui
                                            .selectable_label(self.y_scale == scale, scale.label())
                                            .clicked()
                                        {
                                            self.y_scale = scale;
                                            self.auto_y();
                                        }
                                    }
                                });
                            });

                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("Custom lookup transform (T3)").small().color(p.muted),
                            );
                            ui.horizontal(|ui| {
                                if ui.button("Load lookup table…").clicked() {
                                    match self.open_lookup_loader() {
                                        Ok(points) => {
                                            let applied = self.apply_lookup_to_both();
                                            if applied {
                                                self.set_status(format!(
                                                    "Loaded {points} calibration points; lookup applied to X and Y"
                                                ));
                                            }
                                        }
                                        Err(message) => self.lookup_message = message,
                                    }
                                }
                                let save_clicked = ui.button("Save lookup table…").clicked()
                                    && !self.lookup_table.x().is_empty();
                                if save_clicked
                                    && let Some(path) = rfd::FileDialog::new()
                                        .add_filter("CSV", &["csv"])
                                        .set_file_name("lookup.csv")
                                        .save_file()
                                        && let Err(message) = self.save_lookup(&path) {
                                            self.lookup_message = message;
                                        }
                                if ui.button("Clear lookup").clicked() {
                                    self.clear_lookup();
                                }
                            });
                            if !self.lookup_table.x().is_empty() {
                                let scale_is_lookup = self.x_scale == self.lookup_table.to_scale()
                                    || self.y_scale == self.lookup_table.to_scale();
                                ui.label(
                                    egui::RichText::new(format!(
                                        "lookup table: {} points (source: {})",
                                        self.lookup_table.x().len(),
                                        self.lookup_source.clone().unwrap_or_else(|| "in memory".to_string())
                                    ))
                                    .small()
                                    .color(p.muted),
                                );
                                if scale_is_lookup {
                                    ui.label(egui::RichText::new("Active on axis: X and/or Y").small());
                                }
                            }
                            if !self.lookup_message.is_empty() {
                                ui.weak(&self.lookup_message);
                            }

                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("Autorange rules (T5)").small().color(p.muted),
                            );
                            ui.horizontal(|ui| {
                                if ui.button("Add autorange rule").clicked() {
                                    self.add_autorange_rule();
                                }
                                if ui
                                    .button("Remove selected rule")
                                    .on_hover_text("Remove the selected automatic autorange rule")
                                    .clicked()
                                    && !self.autorange_rules.is_empty()
                                {
                                    let last = self.autorange_rules.len() - 1;
                                    self.remove_autorange_rule(last);
                                }
                            });
                            for i in 0..self.autorange_rules.len() {
                                let summary = format!(
                                    "{}: {} ({}: {})",
                                    self.autorange_rules[i].name,
                                    self.autorange_rules[i].matcher_label(),
                                    self.autorange_rules[i].limit.label(),
                                    self.autorange_rules[i].priority
                                );
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut self.autorange_rules[i].enabled, "");
                                    ui.label(egui::RichText::new(summary).small());
                                });
                            }
                        });

                        ui.add_space(4.0);
                        if secondary_button(ui, p, "Compensation…")
                            .on_hover_text("Load a spillover matrix and apply it to the data")
                            .clicked()
                        {
                            self.open_compensation();
                        }

                        ui.collapsing("Point & view options", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Dot colour:");
                                ui.color_edit_button_srgba(&mut self.point_color);
                            });
                            ui.checkbox(&mut self.gradient, "Density gradient layering");
                            ui.checkbox(
                                &mut self.overlay_populations,
                                "Overlay other populations",
                            );
                            ui.horizontal(|ui| {
                                ui.label("Density map:");
                                egui::ComboBox::new("density_map", "")
                                    .selected_text(self.density_map.label())
                                    .show_ui(ui, |ui| {
                                        for map in crate::plot::DensityMap::all() {
                                            ui.selectable_value(
                                                &mut self.density_map,
                                                map,
                                                map.label(),
                                            );
                                        }
                                    });
                            });
                            ui.add(
                                egui::Slider::new(&mut self.gate_line_width, 0.5..=6.0)
                                    .text("gate line width"),
                            );
                            ui.checkbox(
                                &mut self.show_median_cross,
                                "Median-cross overlay",
                            );
                            ui.checkbox(&mut self.magnetic_snap, "Magnetic snapping");
                            if self.magnetic_snap {
                                ui.add(
                                    egui::Slider::new(&mut self.magnetic_strength, 0.0..=1.0)
                                        .text("snap strength"),
                                );
                                ui.add(
                                    egui::Slider::new(&mut self.magnetic_radius_frac, 0.005..=0.2)
                                        .text("snap radius (% of span)"),
                                );
                            }
                            ui.collapsing("Gate labels", |ui| {
                                ui.checkbox(
                                    &mut self.gate_labels.show,
                                    "Show population labels",
                                );
                                ui.indent("gate_label_fields", |ui| {
                                    ui.checkbox(&mut self.gate_labels.name, "Name");
                                    ui.checkbox(&mut self.gate_labels.count, "Event count");
                                    ui.checkbox(
                                        &mut self.gate_labels.percent_parent,
                                        "% of parent",
                                    );
                                    ui.checkbox(
                                        &mut self.gate_labels.percent_total,
                                        "% of total",
                                    );
                                    ui.checkbox(
                                        &mut self.gate_labels.ancestry,
                                        "Ancestry path",
                                    );
                                });
                                ui.checkbox(
                                    &mut self.gate_labels.backgating,
                                    "Backgate ancestor outlines",
                                );
                                ui.checkbox(
                                    &mut self.gate_labels.backgate_events,
                                    "Show excluded parent events",
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label("View:");
                                for mode in [
                                    ViewMode::Dots,
                                    ViewMode::Contour,
                                    ViewMode::Histogram,
                                    ViewMode::Pseudocolor,
                                    ViewMode::Zebra,
                                ] {
                                    if ui
                                        .selectable_label(self.view_mode == mode, mode.label())
                                        .clicked()
                                    {
                                        self.view_mode = mode;
                                    }
                                }
                            });
                            if matches!(self.view_mode, ViewMode::Contour) {
                                ui.add(
                                    egui::Slider::new(&mut self.contour_levels, 3..=100)
                                        .text("contour levels"),
                                );
                            }
                        });

                        ui.collapsing("Graph controls", |ui| {
                            ui.add(
                                egui::Slider::new(&mut self.plot_smoothing, 0.0..=1.0)
                                    .text("density smoothing"),
                            );
                            ui.horizontal(|ui| {
                                ui.label("Resolution:");
                                egui::ComboBox::new("plot_resolution", "")
                                    .selected_text(format!("{}²", self.plot_resolution))
                                    .show_ui(ui, |ui| {
                                        for res in [64usize, 128, 256, 512] {
                                            ui.selectable_value(
                                                &mut self.plot_resolution,
                                                res,
                                                format!("{res}²"),
                                            );
                                        }
                                    });
                                ui.label("Outliers:");
                                egui::ComboBox::new("outlier_clip", "")
                                    .selected_text(format!("{}%", self.outlier_clip_percent))
                                    .show_ui(ui, |ui| {
                                        for pct in [0.0f64, 1.0, 5.0, 10.0] {
                                            ui.selectable_value(
                                                &mut self.outlier_clip_percent,
                                                pct,
                                                format!("{pct}%"),
                                            );
                                        }
                                    });
                            });
                            ui.checkbox(&mut self.large_dots, "Large dots");
                            ui.checkbox(&mut self.show_grid, "Background grid");
                            ui.checkbox(&mut self.show_medians, "Median lines");
                            ui.checkbox(&mut self.show_event_numbers, "Event numbers");
                            ui.checkbox(&mut self.show_axis_histograms, "Axis histograms");
                            ui.checkbox(
                                &mut self.show_names_frequencies,
                                "Names & frequencies in overlay legend",
                            );
                            ui.horizontal(|ui| {
                                ui.label("Tick notation:");
                                egui::ComboBox::new("tick_notation", "")
                                    .selected_text(self.tick_notation.label())
                                    .show_ui(ui, |ui| {
                                        for notation in crate::plot::TickNotation::all() {
                                            ui.selectable_value(
                                                &mut self.tick_notation,
                                                notation,
                                                notation.label(),
                                            );
                                        }
                                    });
                            });
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.show_axis_labels, "Axis labels");
                                ui.checkbox(&mut self.show_ticks, "Ticks");
                                ui.checkbox(&mut self.show_numbers, "Numbers");
                            });
                            ui.checkbox(
                                &mut self.transpose,
                                "Transpose X/Y axes (display only)",
                            );
                            ui.horizontal(|ui| {
                                ui.label("Y range:");
                                let mut changed = false;
                                changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut self.y_range.0)
                                            .speed((self.y_data_range.1 - self.y_data_range.0).abs().max(1.0) / 100.0),
                                    )
                                    .changed();
                                ui.label("to");
                                changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut self.y_range.1)
                                            .speed((self.y_data_range.1 - self.y_data_range.0).abs().max(1.0) / 100.0),
                                    )
                                    .changed();
                                if changed && self.y_range.0 > self.y_range.1 {
                                    std::mem::swap(&mut self.y_range.0, &mut self.y_range.1);
                                }
                                if ui.small_button("Auto").on_hover_text("Fit Y to the data").clicked() {
                                    self.y_range = normalize_span(
                                        self.y_data_range.0,
                                        self.y_data_range.1,
                                    );
                                }
                            });
                        });

                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("New gate type").small().color(p.muted));
                        for gt in NewGateType::ALL {
                            ui.radio_value(&mut self.new_gate_type, gt, gt.label());
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if primary_button(ui, p, "+ Add Gate").clicked() {
                                self.add_default_gate();
                            }
                            if secondary_button(ui, p, "Remove selected").clicked()
                                && let Some(i) = self.selected_gate
                            {
                                self.remove_gate(i);
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            let selected = self.selected_gate;
                            if secondary_button(ui, p, "Copy")
                                .on_hover_text("Copy the selected gate (Ctrl+C)")
                                .clicked()
                                && let Some(i) = selected
                                && let Some(json) = self.copy_gate(i) {
                                    ui.ctx().copy_text(json);
                                }
                            if secondary_button(ui, p, "Paste")
                                .on_hover_text("Paste as a sibling (Ctrl+V)")
                                .clicked()
                            {
                                self.paste_gate();
                            }
                            if secondary_button(ui, p, "Duplicate")
                                .on_hover_text("Duplicate as a sibling (Ctrl+D)")
                                .clicked()
                                && let Some(i) = selected
                            {
                                self.duplicate_gate(i);
                            }
                            if secondary_button(ui, p, "Sync name")
                                .on_hover_text(
                                    "Copy this geometry to every same-named gate on the same parameters",
                                )
                                .clicked()
                                && let Some(i) = selected
                            {
                                self.sync_named_gates(i);
                            }
                        });
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "Tip: drag on the plot to draw; click places a quadrant/bilinear gate.",
                            )
                            .small()
                            .color(p.muted),
                    );
                });
                });
            });

        // ----- Bottom panel: gate editor -----
        let p = self.palette();
        egui::TopBottomPanel::bottom("gate_editor")
            .resizable(true)
            .default_height(200.0)
            // Shared panel frame so the editor card sits the same distance
            // from the window edges as every other card.
            .frame(panel_frame(p))
            .show(ctx, |ui| {
                let sel = self.selected_gate;
                card_frame(p).show(ui, |ui| {
                    card_head(ui, p, "Gate editor", |_| {});
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Selected gate:")
                                .small()
                                .color(p.muted),
                        );
                        if let Some(i) = sel {
                            ui.text_edit_singleline(&mut self.name_edit);
                            if secondary_button(ui, p, "Rename").clicked() {
                                self.gates[i].name = self.name_edit.clone();
                            }
                            let mut color = self.gates[i].color;
                            if ui.color_edit_button_srgba(&mut color).changed() {
                                self.gates[i].color = color;
                            }
                            ui.separator();
                            // Parent selector: only earlier gates are offered, which
                            // keeps the hierarchy acyclic and topologically ordered.
                            let current = self.gates[i].parent;
                            let current_label = current
                                .map(|p| self.gates[p].name.clone())
                                .unwrap_or_else(|| "(root)".to_string());
                            let options: Vec<(usize, String)> =
                                (0..i).map(|p| (p, self.gates[p].name.clone())).collect();
                            let mut new_parent = current;
                            egui::ComboBox::new("parent_sel", "Parent")
                                .selected_text(current_label)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut new_parent, None, "(root)");
                                    for (p, name) in &options {
                                        ui.selectable_value(&mut new_parent, Some(*p), name.clone());
                                    }
                                });
                            if new_parent != current {
                                self.gates[i].parent = new_parent;
                                self.recompute_populations();
                            }
                            // Apply-to-all toggle.
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Apply to all samples:")
                                        .small()
                                        .color(p.muted),
                                );
                                ui.checkbox(&mut self.gates[i].apply_to_all, "")
                                    .on_hover_text(
                                        "Off: this gate applies only to its owner sample. On: it also applies to every other sample that has gating enabled.",
                                    );
                                if let Some(owner_idx) = self.gates[i].owner {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "(created on sample {})",
                                            self.samples[owner_idx].id
                                        ))
                                        .small()
                                        .color(p.muted),
                                    );
                                } else {
                                    ui.label(
                                        egui::RichText::new("(created before any sample loaded)")
                                            .small()
                                            .color(p.muted),
                                    );
                                }
                            });
                        } else {
                            ui.label(
                                egui::RichText::new("none")
                                    .small()
                                    .color(p.muted),
                            );
                        }
                    });

                    let Some(i) = self.selected_gate else {
                        ui.label(
                            egui::RichText::new("No gate selected.")
                                .color(p.muted)
                                .small(),
                        );
                        return;
                    };
                    if i >= self.gates.len() {
                        return;
                    }

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // Numeric editor for the selected gate.
                    let mut new_kind = self.gates[i].kind.clone();
                    let mut changed = false;
                    match &mut new_kind {
                        GateKind::Rect {
                            x_min,
                            x_max,
                            y_min,
                            y_max,
                            angle,
                        }
                        | GateKind::Ellipse {
                            x_min,
                            x_max,
                            y_min,
                            y_max,
                            angle,
                        } => {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("X min:").small().color(p.muted),
                                );
                                changed |= ui
                                    .add(egui::DragValue::new(x_min).speed(1.0))
                                    .changed();
                                ui.label(
                                    egui::RichText::new("X max:").small().color(p.muted),
                                );
                                changed |= ui
                                    .add(egui::DragValue::new(x_max).speed(1.0))
                                    .changed();
                                ui.label(
                                    egui::RichText::new("Y min:").small().color(p.muted),
                                );
                                changed |= ui
                                    .add(egui::DragValue::new(y_min).speed(1.0))
                                    .changed();
                                ui.label(
                                    egui::RichText::new("Y max:").small().color(p.muted),
                                );
                                changed |= ui
                                    .add(egui::DragValue::new(y_max).speed(1.0))
                                    .changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Rotation (deg):")
                                        .small()
                                        .color(p.muted),
                                );
                                let mut deg = angle.to_degrees();
                                if ui
                                    .add(egui::DragValue::new(&mut deg).speed(1.0))
                                    .changed()
                                {
                                    *angle = deg.to_radians();
                                    changed = true;
                                }
                            });
                        }
                        GateKind::Polygon { vertices } => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} vertices — drag a vertex handle on the plot to reshape.",
                                    vertices.len()
                                ))
                                .small()
                                .color(p.muted),
                            );
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Centroid: ({:.1}, {:.1})",
                                        centroid(vertices)[0],
                                        centroid(vertices)[1]
                                    ))
                                    .small()
                                    .color(p.muted),
                                );
                            });
                            ui.horizontal(|ui| {
                                if secondary_button(ui, p, "+ Add vertex").clicked() {
                                    // Insert a new vertex at the midpoint of the longest edge.
                                    if vertices.len() >= 2 {
                                        let mut best_edge = 0usize;
                                        let mut best_len = f64::NEG_INFINITY;
                                        for j in 0..vertices.len() {
                                            let a = vertices[j];
                                            let b = vertices[(j + 1) % vertices.len()];
                                            let len =
                                                ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2))
                                                    .sqrt();
                                            if len > best_len {
                                                best_len = len;
                                                best_edge = j;
                                            }
                                        }
                                        let a = vertices[best_edge];
                                        let b = vertices[(best_edge + 1) % vertices.len()];
                                        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                                        vertices.insert(best_edge + 1, mid);
                                        changed = true;
                                    }
                                }
                                if secondary_button(ui, p, "− Remove vertex").clicked()
                                    && vertices.len() > 3 {
                                        vertices.pop();
                                        changed = true;
                                    }
                                ui.label(
                                    egui::RichText::new("Minimum 3 vertices.")
                                        .small()
                                        .color(p.muted),
                                );
                            });
                        }
                        GateKind::Quadrant { x, y, .. } => {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Split X:").small().color(p.muted),
                                );
                                changed |= ui.add(egui::DragValue::new(x).speed(1.0)).changed();
                                ui.label(
                                    egui::RichText::new("Split Y:").small().color(p.muted),
                                );
                                changed |= ui.add(egui::DragValue::new(y).speed(1.0)).changed();
                            });
                            ui.label(
                                egui::RichText::new(
                                    "Tip: all four quadrant gates share the same split point when drawn together.",
                                )
                                .small()
                                .color(p.muted),
                            );
                        }
                        GateKind::Bilinear {
                            x1,
                            y1,
                            x2,
                            y2,
                            below,
                        } => {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Start X:").small().color(p.muted),
                                );
                                changed |= ui.add(egui::DragValue::new(x1).speed(1.0)).changed();
                                ui.label(
                                    egui::RichText::new("Start Y:").small().color(p.muted),
                                );
                                changed |= ui.add(egui::DragValue::new(y1).speed(1.0)).changed();
                                ui.label(
                                    egui::RichText::new("End X:").small().color(p.muted),
                                );
                                changed |= ui.add(egui::DragValue::new(x2).speed(1.0)).changed();
                                ui.label(
                                    egui::RichText::new("End Y:").small().color(p.muted),
                                );
                                changed |= ui.add(egui::DragValue::new(y2).speed(1.0)).changed();
                            });
                            ui.horizontal(|ui| {
                                changed |= ui
                                    .checkbox(below, "Select points below the line")
                                    .changed();
                                ui.label(
                                    egui::RichText::new("(uncheck to select the region above)")
                                        .small()
                                        .color(p.muted),
                                );
                            });
                        }
                        GateKind::Mask { .. } => {}
                    }
                    if changed {
                        let old_kind = self.gates[i].kind.clone();
                        self.gates[i].kind = new_kind;
                        let new_kind_value = self.gates[i].kind.clone();
                        self.commit_gate_edit(i, &old_kind, &new_kind_value);
                        self.recompute_populations();
                    }

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // Non-interactive transform buttons.
                    let step = (self.x_range.1 - self.x_range.0).abs().max(1.0) * 0.02;
                    ui.label(
                        egui::RichText::new("Transform:").small().color(p.muted),
                    );
                    ui.horizontal(|ui| {
                        let mut t = false;
                        if secondary_button(ui, p, "⬅").on_hover_text("Move left").clicked() {
                            self.nudge_gate(i, -step, 0.0);
                            t = true;
                        }
                        if secondary_button(ui, p, "➡").on_hover_text("Move right").clicked() {
                            self.nudge_gate(i, step, 0.0);
                            t = true;
                        }
                        if secondary_button(ui, p, "⬆").on_hover_text("Move up").clicked() {
                            self.nudge_gate(i, 0.0, step);
                            t = true;
                        }
                        if secondary_button(ui, p, "⬇").on_hover_text("Move down").clicked() {
                            self.nudge_gate(i, 0.0, -step);
                            t = true;
                        }
                        if secondary_button_hover(ui, p, "⟲", "Rotate -5°").clicked() {
                            let c = self.gates[i].kind.center();
                            self.rotate_gate(i, c, (-5.0f64).to_radians());
                            t = true;
                        }
                        if secondary_button_hover(ui, p, "⟳", "Rotate +5°").clicked() {
                            let c = self.gates[i].kind.center();
                            self.rotate_gate(i, c, (5.0f64).to_radians());
                            t = true;
                        }
                        if secondary_button_hover(ui, p, "➕", "Enlarge").clicked() {
                            let c = self.gates[i].kind.center();
                            self.scale_gate(i, c, 1.1);
                            t = true;
                        }
                        if secondary_button_hover(ui, p, "➖", "Shrink").clicked() {
                            let c = self.gates[i].kind.center();
                            self.scale_gate(i, c, 0.9);
                            t = true;
                        }
                        if t {
                            self.recompute_populations();
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Convert to:").small().color(p.muted),
                        );
                        for target in [
                            crate::gating::GateShape::Rectangle,
                            crate::gating::GateShape::Ellipse,
                            crate::gating::GateShape::Polygon,
                        ] {
                            if secondary_button(ui, p, target.label()).clicked() {
                                self.convert_gate(i, target);
                            }
                        }
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                    let count = self.pop_indices.get(i).map(|v| v.len()).unwrap_or(0);
                    let parent_count = self.gates[i]
                        .parent
                        .and_then(|p| self.pop_indices.get(p).map(|v| v.len()));
                    let total = self.current_fcs().map(|f| f.event_count).unwrap_or(0);
                    let pct_text = gate_percentage_label(count, parent_count, total);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "Events in gate: {count} ({pct_text})"
                            ))
                            .small()
                            .color(p.muted),
                        );
                        if primary_button(ui, p, "Open detached window").clicked() {
                            self.open_popout(i);
                        }
                    });
                });
            });

        // ----- Right panel: population card + cross-sample summary -----
        let p = self.palette();
        egui::SidePanel::right("populations")
            .default_width(330.0)
            // Shared panel frame so the population cards sit the same
            // distance from the window edges as every other card.
            .frame(panel_frame(p))
            .show(ctx, |ui| {
                // --- Population card ---
                card_frame(p).show(ui, |ui| {
                    let sel = self.selected_gate;
                    let total = self.current_fcs().map(|f| f.event_count).unwrap_or(0);
                    let counts: Vec<usize> = self.pop_indices.iter().map(|v| v.len()).collect();

                    let (title, subtitle, count, pct_parent, pct_total) = match sel {
                        Some(i) if i < self.gates.len() => {
                            let gate = &self.gates[i];
                            let parent_count = gate
                                .parent
                                .and_then(|p| counts.get(p).copied())
                                .unwrap_or(total);
                            let c = counts.get(i).copied().unwrap_or(0);
                            let pcts = gate_percentages(c, Some(parent_count), total);
                            (
                                gate.name.clone(),
                                gate.parent
                                    .map(|p| self.gates[p].name.clone())
                                    .unwrap_or_else(|| "(root)".to_string()),
                                c,
                                pcts.of_parent.unwrap_or(0.0),
                                pcts.of_total,
                            )
                        }
                        _ => {
                            let c = if sel.is_none() { total } else { 0 };
                            (
                                "All events".to_string(),
                                "Ungated population".to_string(),
                                c,
                                100.0,
                                100.0,
                            )
                        }
                    };

                    card_head(ui, p, "Population", |ui| {
                        ui.label(
                            egui::RichText::new(if sel.is_some() { "selected" } else { "root" })
                                .small()
                                .color(p.muted),
                        );
                    });
                    section_label(ui, p, &subtitle);
                    ui.add_space(6.0);

                    ui.label(egui::RichText::new(&title).strong().color(p.ink));
                    ui.add_space(6.0);

                    // Metric strip: Events / % parent / % total.
                    ui.horizontal(|ui| {
                        metric_cell(ui, p, "Events", &format!("{count}"));
                        ui.add_space(8.0);
                        metric_cell(ui, p, "% parent", &format!("{pct_parent:.1}"));
                        ui.add_space(8.0);
                        metric_cell(ui, p, "% total", &format!("{pct_total:.1}"));
                    });
                    ui.add_space(8.0);

                    // Gate settings.
                    if let Some(i) = sel {
                        if i < self.gates.len() {
                            ui.label(egui::RichText::new("Gate settings").small().color(p.muted));
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label("Name:");
                                ui.text_edit_singleline(&mut self.name_edit);
                                if ui.button("Rename").clicked() {
                                    self.gates[i].name = self.name_edit.clone();
                                }
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                // visibility isn't stored per-gate in flowers.
                                let _gate = &self.gates[i];
                                let _visible = true;
                                if ui.button("Open detached window").clicked() {
                                    self.open_popout(i);
                                }
                                if ui.button("Delete").clicked() {
                                    self.remove_gate(i);
                                }
                            });
                        }
                    } else {
                        ui.label(
                            egui::RichText::new("Select a population to view its settings.")
                                .color(p.muted)
                                .small(),
                        );
                    }
                });

                ui.add_space(12.0);

                // --- Populations across samples ---
                card_frame(p).show(ui, |ui| {
                    card_head(ui, p, "Cross-samples", |_| {});
                    section_label(ui, p, "Counts per enabled sample");
                    ui.add_space(6.0);

                    if self.samples.is_empty() || self.gates.is_empty() {
                        ui.label(
                            egui::RichText::new("Load samples and create gates.")
                                .color(p.muted)
                                .small(),
                        );
                    } else {
                        egui::Grid::new("cross_sample")
                            .num_columns(5)
                            .spacing([8.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                for heading in ["Sample", "Pop", "Count", "% total", "% parent"] {
                                    ui.label(
                                        egui::RichText::new(heading)
                                            .small()
                                            .strong()
                                            .color(p.muted),
                                    );
                                }
                                ui.end_row();
                                for (sample_idx, sample) in self.samples.iter().enumerate() {
                                    let pops = compute_gate_populations(
                                        &sample.fcs.data,
                                        &self.gates,
                                        Some(sample_idx),
                                        sample.apply_gates,
                                    );
                                    for (i, gate) in self.gates.iter().enumerate() {
                                        let c = pops.get(i).map(|v| v.len()).unwrap_or(0);
                                        let parent_count =
                                            gate.parent.and_then(|p| pops.get(p).map(|v| v.len()));
                                        let pcts = gate_percentages(
                                            c,
                                            parent_count,
                                            sample.fcs.event_count,
                                        );
                                        let parent_pct = pcts
                                            .of_parent
                                            .map(|v| format!("{v:.1}%"))
                                            .unwrap_or_else(|| "—".to_string());
                                        ui.label(egui::RichText::new(&sample.id).small());
                                        ui.label(egui::RichText::new(&gate.name).small());
                                        ui.label(egui::RichText::new(format!("{c}")).small());
                                        ui.label(
                                            egui::RichText::new(format!("{:.1}%", pcts.of_total))
                                                .small(),
                                        );
                                        ui.label(egui::RichText::new(parent_pct).small());
                                        ui.end_row();
                                    }
                                }
                            });
                    }
                    ui.add_space(6.0);
                    if ui.button("Export CSV…").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("CSV", &["csv"])
                            .set_file_name("populations.csv")
                            .save_file()
                    {
                        self.export_csv(&path);
                    }
                });
            });

        /// A single metric cell: small caps label + value.
        fn metric_cell(ui: &mut egui::Ui, p: Palette, label: &str, value: &str) {
            egui::Frame::none()
                .fill(p.soft)
                .rounding(6.0)
                .inner_margin(egui::Margin::same(6.0))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(label).small().color(p.muted));
                        ui.label(egui::RichText::new(value).strong().color(p.ink));
                    });
                });
        }

        // ----- Central panel: plot card -----
        let p = self.palette();
        egui::CentralPanel::default()
            .frame(panel_frame(p))
            .show(ctx, |ui| {
                if self.current_fcs().is_none() {
                    card_frame(p).show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Load an FCS file to begin")
                                    .color(p.muted)
                                    .small(),
                            );
                            ui.label(
                                egui::RichText::new(
                                    "Use File ▸ Load FCS, or click Import FCS on the left.",
                                )
                                .color(p.muted)
                                .small(),
                            );
                            // Placeholder scatter-plot image so the empty state
                            // reads as a plot instead of a plain box.
                            let image_rect = ui.available_rect_before_wrap();
                            let painter = ui.painter();
                            let center = image_rect.center();
                            painter.circle_filled(
                                egui::pos2(center.x, center.y - 24.0),
                                16.0,
                                p.teal,
                            );
                            painter.circle_filled(
                                egui::pos2(center.x - 7.0, center.y - 12.0),
                                5.0,
                                egui::Color32::from_rgb(56, 189, 248),
                            );
                            painter.circle_filled(
                                egui::pos2(center.x + 7.0, center.y - 6.0),
                                5.0,
                                egui::Color32::from_rgb(251, 191, 36),
                            );
                            painter.circle_filled(
                                egui::pos2(center.x - 4.0, center.y + 4.0),
                                5.0,
                                egui::Color32::from_rgb(248, 113, 113),
                            );
                            ui.add_space(8.0);
                        });
                    });
                    return;
                }
                if self.x_param.is_none() || self.y_param.is_none() {
                    card_frame(p).show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Select X and Y parameters to plot")
                                    .color(p.muted)
                                    .small(),
                            );
                            ui.add_space(8.0);
                        });
                    });
                    return;
                }
                if self.plot_points.is_empty() {
                    self.update_plot_points();
                }

                let cfg = self.main_plot_cfg();

                card_frame(p).show(ui, |ui| {
                    // --- Plot header: title + tools ---
                    let sample_name = self
                        .current_sample
                        .and_then(|i| self.samples.get(i))
                        .map(|s| s.id.clone())
                        .unwrap_or_default();
                    let comp_status = {
                        let mut status = if self.gates.is_empty() {
                            "no gates".to_string()
                        } else {
                            format!("{} gate(s)", self.gates.len())
                        };
                        if self.current_sample_compensated() {
                            status.push_str(" · compensated");
                        }
                        status.push_str(" · ");
                        status.push_str(&self.compensation_node_status());
                        status
                    };
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(&sample_name).strong().color(p.ink));
                            ui.label(egui::RichText::new(comp_status).small().color(p.muted));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Save plot (rightmost).
                            if secondary_button(ui, p, "Save PNG").clicked() {
                                let mut dlg = rfd::FileDialog::new().set_file_name("plot.png");
                                for &(name, exts) in ImageFormat::dialog_filters() {
                                    dlg = dlg.add_filter(name, exts);
                                }
                                if let Some(path) = dlg.save_file() {
                                    self.export_image(&path);
                                }
                            }
                            ui.add_space(6.0);
                            // Clear / finish.
                            if secondary_button(ui, p, "Clear").clicked() {
                                // Cancel any in-progress draw.
                                self.interact.drag_start = None;
                                self.interact.drag_current = None;
                            }
                            ui.add_space(6.0);
                            // Tool toggle group: select / rectangle / polygon.
                            egui::Frame::none()
                                .fill(p.soft)
                                .stroke(egui::Stroke::new(1.0_f32, p.border))
                                .rounding(6.0)
                                .inner_margin(egui::Margin::same(2.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let rect_active =
                                            matches!(self.new_gate_type, NewGateType::Rect);
                                        let ellipse_active =
                                            matches!(self.new_gate_type, NewGateType::Ellipse);
                                        let poly_active =
                                            matches!(self.new_gate_type, NewGateType::Polygon);
                                        let quad_active =
                                            matches!(self.new_gate_type, NewGateType::Quadrant);
                                        let line_active =
                                            matches!(self.new_gate_type, NewGateType::Bilinear);
                                        if tool_button(ui, p, "Rectangle", rect_active).clicked() {
                                            self.new_gate_type = NewGateType::Rect;
                                        }
                                        if tool_button(ui, p, "Ellipse", ellipse_active).clicked() {
                                            self.new_gate_type = NewGateType::Ellipse;
                                        }
                                        if tool_button(ui, p, "Polygon", poly_active).clicked() {
                                            self.new_gate_type = NewGateType::Polygon;
                                        }
                                        if tool_button(ui, p, "Quadrant", quad_active).clicked() {
                                            self.new_gate_type = NewGateType::Quadrant;
                                        }
                                        if tool_button(ui, p, "Bilinear", line_active).clicked() {
                                            self.new_gate_type = NewGateType::Bilinear;
                                        }
                                    });
                                });
                        });
                    });
                    ui.add_space(8.0);

                    // --- Plot area ---
                    // Reserve room for the footer row before the scroll area
                    // claims the remaining height. Without this the plot fills
                    // every pixel left in the card and pushes the footer (and
                    // the card's bottom border) past the bottom of the window.
                    let footer_height = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                    let plot_height = (ui.available_height() - footer_height - 8.0).max(0.0);
                    egui::ScrollArea::both()
                        .max_height(plot_height)
                        .show(ui, |ui| {
                            let mut inter = PlotInteraction {
                                drag_start: self.interact.drag_start,
                                drag_current: self.interact.drag_current,
                                ..Default::default()
                            };
                            show_plot(ui, "scatter_plot", &cfg, &mut inter);

                            let tol = 0.02
                                * (self.x_range.1 - self.x_range.0)
                                    .abs()
                                    .max((self.y_range.1 - self.y_range.0).abs())
                                    .max(1.0);
                            let outcome = drive_interact(
                                &mut self.interact,
                                &inter,
                                &self.gates,
                                self.selected_gate,
                                self.new_gate_type,
                                tol,
                            );
                            if let Some(outcome) = outcome {
                                match outcome {
                                    InteractOutcome::Create { start, end } => {
                                        self.create_gate_from_drag(start, end)
                                    }
                                    InteractOutcome::Update { gate, kind } => {
                                        self.selected_gate = Some(gate);
                                        self.sync_name_edit();
                                        let old = self.gates.get(gate).map(|g| g.kind.clone());
                                        if gate < self.gates.len() {
                                            self.gates[gate].kind = kind;
                                        }
                                        if let Some(old) = old {
                                            let new_kind =
                                                self.gates.get(gate).map(|g| g.kind.clone());
                                            if let Some(new_kind) = new_kind {
                                                self.commit_gate_edit(gate, &old, &new_kind);
                                            }
                                        }
                                        self.recompute_populations();
                                    }
                                    InteractOutcome::Select(gate) => {
                                        self.selected_gate = Some(gate);
                                        self.sync_name_edit();
                                    }
                                    InteractOutcome::Click(p) => self.create_click_gate(p, None),
                                }
                            }
                        });
                    ui.add_space(8.0);

                    // Density-map legend for pseudocolour / zebra views.
                    if matches!(self.view_mode, ViewMode::Pseudocolor | ViewMode::Zebra) {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("density").small().color(p.muted));
                            ui.label(egui::RichText::new("low").small().color(p.muted));
                            density_legend_ui(ui, &self.density_map);
                            ui.label(egui::RichText::new("high").small().color(p.muted));
                        });
                    }

                    // --- Plot footer: hints + options ---
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Draw hints").small().color(p.muted));
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new("drag").small().strong().color(p.ink));
                        ui.label(" to draw a gate  ");
                        ui.label(egui::RichText::new("click").small().strong().color(p.ink));
                        ui.label(" to place a quadrant/bilinear gate  ");
                        if self.transpose {
                            ui.label(
                                egui::RichText::new("axes transposed — gate editing paused")
                                    .small()
                                    .color(p.muted),
                            );
                        }
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new("X").small().strong().color(p.muted));
                        if ui.small_button("−").on_hover_text("Zoom X out").clicked() {
                            zoom_axis_range(&mut self.x_range, &self.x_scale, 1.25);
                        }
                        if ui.small_button("+").on_hover_text("Zoom X in").clicked() {
                            zoom_axis_range(&mut self.x_range, &self.x_scale, 0.8);
                        }
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new("Y").small().strong().color(p.muted));
                        if ui.small_button("−").on_hover_text("Zoom Y out").clicked() {
                            zoom_axis_range(&mut self.y_range, &self.y_scale, 1.25);
                        }
                        if ui.small_button("+").on_hover_text("Zoom Y in").clicked() {
                            zoom_axis_range(&mut self.y_range, &self.y_scale, 0.8);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.checkbox(
                                &mut self.gradient,
                                egui::RichText::new("density gradient")
                                    .small()
                                    .color(p.muted),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(
                                    self.pop_indices
                                        .iter()
                                        .map(|v| v.len())
                                        .sum::<usize>()
                                        .to_string(),
                                )
                                .small()
                                .color(p.muted),
                            );
                            ui.label(" events drawn");
                        });
                    });
                    // Regression-test hook: remember the plot card's content and
                    // clip rects so tests can assert the card is not cut off at
                    // the bottom of the central panel.
                    #[cfg(test)]
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(
                            egui::Id::new("central_plot_card_rects"),
                            (ui.min_rect(), ui.clip_rect()),
                        );
                    });
                });
            });

        // ----- Layout editor window -----
        self.layout_window(ctx);

        // ----- Detached graph windows (V7) -----
        let mut wi = 0;
        while wi < self.popouts.len() {
            let mut st = std::mem::take(&mut self.popouts[wi]);
            if st.open {
                self.popout_window(ctx, wi, &mut st);
            }
            self.popouts[wi] = st;
            if !self.popouts[wi].open {
                self.popouts.remove(wi);
                if let Some(active) = self.active_popout {
                    if active == wi {
                        self.active_popout = None;
                    } else if active > wi {
                        self.active_popout = Some(active - 1);
                    }
                }
            } else {
                wi += 1;
            }
        }

        // ----- Compensation editor -----
        if self.comp_open {
            enum CompAction {
                LoadCsv,
                Spillover,
                Identity,
                Invert,
                AddChannels,
                ApplyCurrent,
                ApplyAll,
                ClearCurrent,
                ClearAll,
                ReapplyAcquisition,
            }
            let p = self.palette();
            let mut open = self.comp_open;
            let mut action: Option<CompAction> = None;
            // Temporarily move the draft out so the window closure only needs
            // an immutable borrow of the rest of the app state.
            let mut draft = self.comp_draft.take();
            let current_sample_text = self
                .current_sample
                .and_then(|i| self.samples.get(i))
                .map(|s| {
                    format!(
                        "{} ({})",
                        s.id,
                        if s.compensation.is_some() {
                            "compensated"
                        } else {
                            "uncompensated"
                        }
                    )
                })
                .unwrap_or_else(|| "no sample selected".to_string());
            let sample_status = self
                .current_fcs()
                .map(|fcs| (parameter_names(fcs), parameter_descriptions(fcs)));

            egui::Window::new("Compensation")
                .open(&mut open)
                .resizable(true)
                .default_size([820.0, 580.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Sample:")
                                .small()
                                .color(p.muted),
                        );
                        ui.label(egui::RichText::new(&current_sample_text).small());
                        if let (Some(matrix), Some((names, descriptions))) =
                            (draft.as_ref(), sample_status.as_ref())
                        {
                            ui.separator();
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} / {} matrix channels match",
                                    matrix.matched_count(names, descriptions),
                                    matrix.channels.len()
                                ))
                                .small()
                                .color(p.muted),
                            );
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        if secondary_button(ui, p, "Load matrix CSV…").clicked() {
                            action = Some(CompAction::LoadCsv);
                        }
                        if secondary_button(ui, p, "Use $SPILLOVER")
                            .on_hover_text("Load the acquisition spillover matrix from the FCS file")
                            .clicked()
                        {
                            action = Some(CompAction::Spillover);
                        }
                        if secondary_button(ui, p, "New identity").clicked() {
                            action = Some(CompAction::Identity);
                        }
                        if secondary_button(ui, p, "Invert")
                            .on_hover_text("Invert the matrix (spillover → compensation)")
                            .clicked()
                        {
                            action = Some(CompAction::Invert);
                        }
                        if secondary_button(ui, p, "Add all sample channels").clicked() {
                            action = Some(CompAction::AddChannels);
                        }
                    });
                    ui.separator();

                    match &mut draft {
                        Some(matrix) => {
                            ui.label(
                                egui::RichText::new(
                                    "Rows = source channel, columns = corrected channel. Values are applied as data × matrix.",
                                )
                                .small()
                                .color(p.muted),
                            );
                            ui.add_space(4.0);
                            egui::ScrollArea::both()
                                .max_height(320.0)
                                .show(ui, |ui| {
                                    egui::Grid::new("compensation_matrix_grid")
                                        .striped(true)
                                        .spacing([6.0, 3.0])
                                        .show(ui, |ui| {
                                            ui.label(
                                                egui::RichText::new("channel")
                                                    .small()
                                                    .strong(),
                                            );
                                            for channel in &matrix.channels {
                                                ui.label(
                                                    egui::RichText::new(channel)
                                                        .small()
                                                        .strong(),
                                                )
                                                .on_hover_text(channel);
                                            }
                                            ui.end_row();
                                            for i in 0..matrix.channels.len() {
                                                ui.label(
                                                    egui::RichText::new(
                                                        &matrix.channels[i],
                                                    )
                                                    .small(),
                                                )
                                                .on_hover_text(&matrix.channels[i]);
                                                for j in 0..matrix.channels.len() {
                                                    if let Some(value) =
                                                        matrix.values.get_mut(i).and_then(|r| r.get_mut(j))
                                                    {
                                                        ui.add(
                                                            egui::DragValue::new(value)
                                                                .speed(0.01)
                                                                .max_decimals(4),
                                                        );
                                                    }
                                                }
                                                ui.end_row();
                                            }
                                        });
                                });
                        }
                        None => {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(
                                    "No matrix loaded. Load a CSV, use the file's $SPILLOVER, or create an identity matrix.",
                                )
                                .color(p.muted),
                            );
                            ui.add_space(8.0);
                        }
                    }

                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        if primary_button(ui, p, "Apply to current").clicked() {
                            action = Some(CompAction::ApplyCurrent);
                        }
                        if primary_button(ui, p, "Apply to all samples").clicked() {
                            action = Some(CompAction::ApplyAll);
                        }
                        ui.add_space(8.0);
                        if secondary_button(ui, p, "Reapply acquisition matrix").clicked() {
                            action = Some(CompAction::ReapplyAcquisition);
                        }
                        if secondary_button(ui, p, "Remove current").clicked() {
                            action = Some(CompAction::ClearCurrent);
                        }
                        if secondary_button(ui, p, "Remove all").clicked() {
                            action = Some(CompAction::ClearAll);
                        }
                        if secondary_button(ui, p, "Toggle node visibility").clicked() {
                            self.toggle_compensation_node();
                        }
                    });
                    if !self.comp_message.is_empty() {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(&self.comp_message)
                                .small()
                                .color(p.muted),
                        );
                    }
                });

            self.comp_open = open;
            self.comp_draft = draft;

            match action {
                Some(CompAction::LoadCsv) => {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Compensation matrix", &["csv", "tsv", "txt"])
                        .pick_file()
                    {
                        match std::fs::read_to_string(&path) {
                            Ok(text) => match CompensationMatrix::from_csv(&text) {
                                Ok(matrix) => {
                                    self.comp_message = format!(
                                        "Loaded {} ({} × {} channels) from {}.",
                                        matrix.channels.len(),
                                        matrix.channels.len(),
                                        matrix.values.len(),
                                        path.display()
                                    );
                                    self.comp_draft = Some(matrix);
                                }
                                Err(e) => {
                                    self.comp_message = format!("Could not parse matrix: {e}")
                                }
                            },
                            Err(e) => {
                                self.comp_message = format!("Could not read matrix file: {e}")
                            }
                        }
                    }
                }
                Some(CompAction::Spillover) => self.load_acquisition_matrix(),
                Some(CompAction::Identity) => self.comp_identity(),
                Some(CompAction::Invert) => {
                    if let Some(matrix) = self.comp_draft.take() {
                        match matrix.inverted() {
                            Ok(inverted) => {
                                self.comp_draft = Some(inverted);
                                self.comp_message = "Matrix inverted.".to_string();
                            }
                            Err(e) => {
                                self.comp_draft = Some(matrix);
                                self.comp_message = format!("Could not invert: {e}");
                            }
                        }
                    }
                }
                Some(CompAction::AddChannels) => self.comp_add_missing_channels(),
                Some(CompAction::ApplyCurrent) => self.apply_compensation(false),
                Some(CompAction::ApplyAll) => self.apply_compensation(true),
                Some(CompAction::ClearCurrent) => self.clear_compensation(false),
                Some(CompAction::ClearAll) => self.clear_compensation(true),
                Some(CompAction::ReapplyAcquisition) => {
                    let matrix = self.current_fcs().and_then(acquisition_compensation);
                    if let Some(matrix) = matrix {
                        self.comp_draft = Some(matrix.clone());
                        self.apply_compensation(false);
                    } else {
                        self.comp_message =
                            "This sample has no usable $SPILLOVER keyword.".to_string();
                    }
                }
                None => {}
            }
        }

        // ----- "Apply existing gates to this file?" prompt -----
        if let Some(idx) = self.pending_apply_prompt {
            if idx >= self.samples.len() {
                self.pending_apply_prompt = None;
            } else {
                let name = self.samples[idx].id.clone();
                let n = self.gates.len();
                let mut choice: Option<bool> = None;
                egui::Window::new("Apply gates to new file?")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(format!("Loaded \"{name}\"."));
                        ui.label(format!(
                            "Apply the existing {n} gate(s) to this file?"
                        ));
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui.button("Apply gates").clicked() {
                                choice = Some(true);
                            }
                            if ui.button("Don't apply").clicked() {
                                choice = Some(false);
                            }
                        });
                        ui.weak("You can change this later with the \"gates\" checkbox in the sample list.");
                    });
                if let Some(apply) = choice {
                    if let Some(s) = self.samples.get_mut(idx) {
                        s.apply_gates = apply;
                    }
                    self.pending_apply_prompt = None;
                    self.recompute_populations();
                }
            }
        }

        // ----- Undo history -----
        // Record once per frame (coalescing drags into a single step).
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        self.record_history(pointer_down);
    }
}

impl eframe::App for FlowersApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcs::{FcsFile, Parameter};
    use ndarray::array;
    use std::collections::HashMap;

    fn fcs_with(names: &[&str]) -> FcsFile {
        let parameters = names
            .iter()
            .map(|n| Parameter {
                name: (*n).to_string(),
                description: (*n).to_string(),
                range: 1.0,
                exponent: 0.0,
                min: 0.0,
                max: 1.0,
            })
            .collect();
        FcsFile {
            version: "FCS3.1".to_string(),
            keywords: HashMap::new(),
            parameters,
            event_count: 0,
            data: ndarray::Array2::zeros((0, names.len())),
        }
    }

    #[test]
    fn gate_percentages_use_parent_and_total_denominators() {
        let nested = gate_percentages(50, Some(200), 1000);
        assert!((nested.of_parent.unwrap() - 25.0).abs() < 1e-9);
        assert!((nested.of_total - 5.0).abs() < 1e-9);

        let root = gate_percentages(200, None, 1000);
        assert!(root.of_parent.is_none());
        assert!((root.of_total - 20.0).abs() < 1e-9);

        // Empty denominators are reported as zero rather than NaN.
        let empty = gate_percentages(0, Some(0), 0);
        assert_eq!(empty.of_parent, Some(0.0));
        assert_eq!(empty.of_total, 0.0);

        // Nested gates display both denominators; root gates show only total.
        assert_eq!(
            gate_percentage_label(50, Some(200), 1000),
            "25.0% parent, 5.0% total"
        );
        assert_eq!(gate_percentage_label(200, None, 1000), "20.0% total");
    }

    #[test]
    fn nested_gate_population_reports_parent_and_total_percentages() {
        let data = array![[0.0_f32, 0.0], [1.0, 1.0], [2.0, 2.0], [10.0, 10.0],];
        let rect = |min, max| GateKind::Rect {
            x_min: min,
            x_max: max,
            y_min: min,
            y_max: max,
            angle: 0.0,
        };
        let gates = vec![
            GateDef {
                name: "Live".into(),
                x_idx: 0,
                y_idx: 1,
                parent: None,
                kind: rect(-0.5, 5.0),
                color: egui::Color32::WHITE,
                owner: None,
                apply_to_all: true,
            },
            GateDef {
                name: "Sub".into(),
                x_idx: 0,
                y_idx: 1,
                parent: Some(0),
                kind: rect(0.5, 1.5),
                color: egui::Color32::WHITE,
                owner: None,
                apply_to_all: true,
            },
        ];

        let pops = compute_gate_populations(&data, &gates, None, false);
        assert_eq!(pops[0].len(), 3);
        assert_eq!(pops[1].len(), 1);

        let live = gate_percentages(pops[0].len(), None, data.nrows());
        assert!((live.of_total - 75.0).abs() < 1e-9);

        let sub = gate_percentages(pops[1].len(), Some(pops[0].len()), data.nrows());
        assert!((sub.of_parent.unwrap() - (1.0 / 3.0) * 100.0).abs() < 1e-9);
        assert!((sub.of_total - 25.0).abs() < 1e-9);
    }

    #[test]
    fn default_axes_prefer_fsc_h_and_ssc_h() {
        let fcs = fcs_with(&["FSC-A", "SSC-A", "FSC-H", "SSC-H", "CD3"]);
        assert_eq!(default_param_pair(&fcs), (2, 3));

        // Lazily falls back to FSC-A / SSC-A.
        let fcs = fcs_with(&["FSC-A", "SSC-A", "CD3"]);
        assert_eq!(default_param_pair(&fcs), (0, 1));

        // No FSC/SSC channels: use the first two parameters.
        let fcs = fcs_with(&["A", "B", "C"]);
        assert_eq!(default_param_pair(&fcs), (0, 1));

        // Single parameter: both axes clamp to index 0.
        let fcs = fcs_with(&["FSC-H"]);
        assert_eq!(default_param_pair(&fcs), (0, 0));
    }

    #[test]
    fn edit_number_formatting_round_trips() {
        assert_eq!(format_edit_num(0.0), "0");
        assert_eq!(format_edit_num(7_000_000.0), "7.00e6");
        assert_eq!(format_edit_num(5_300_871.0), "5.30e6");
        assert_eq!(format_edit_num(123.0), "123");
        assert_eq!(format_edit_num(1.5), "1.5");
        assert_eq!(format_edit_num(1e-4), "1.00e-4");
        // The shown text must parse back to exactly the original value for
        // mid-range values that are not rounded by the 3-sig-fig branch.
        for v in [0.0, 123.0, 1.5, 42.0, 0.015] {
            let s = format_edit_num(v);
            let back: f64 = s.parse().unwrap_or_else(|_| panic!("{s} did not parse"));
            assert_eq!(back, v, "{v} -> {s} -> {back}");
        }
    }

    #[test]
    fn zoom_axis_range_keeps_the_center_and_respects_log_axes() {
        let mut linear = (0.0, 10.0);
        zoom_axis_range(&mut linear, &AxisScale::Linear, 0.5);
        assert!((linear.0 - 2.5).abs() < 1e-9);
        assert!((linear.1 - 7.5).abs() < 1e-9);

        // Zooming out by the reciprocal restores the original range.
        zoom_axis_range(&mut linear, &AxisScale::Linear, 2.0);
        assert!((linear.0 - 0.0).abs() < 1e-9);
        assert!((linear.1 - 10.0).abs() < 1e-9);

        // On a log axis the centre is the geometric mean, so a 0.5 zoom of
        // 1..100 stays centred on 10.
        let mut log = (1.0, 100.0);
        zoom_axis_range(&mut log, &AxisScale::Log, 0.5);
        assert!((log.0 - 10f64.powf(0.5)).abs() < 1e-9);
        assert!((log.1 - 10f64.powf(1.5)).abs() < 1e-9);
        assert!((log.0 * log.1 - 100.0).abs() < 1e-6);

        // Degenerate input must leave the range untouched.
        let mut unchanged = (0.0, 0.0);
        zoom_axis_range(&mut unchanged, &AxisScale::Linear, 0.0);
        assert_eq!(unchanged, (0.0, 0.0));
        let mut non_finite = (f64::NAN, 1.0);
        zoom_axis_range(&mut non_finite, &AxisScale::Linear, 0.5);
        assert!(non_finite.0.is_nan());
        assert_eq!(non_finite.1, 1.0);
    }

    #[test]
    fn axis_scale_primary_offers_linear_and_log_only() {
        let primary = AxisScale::primary();
        assert_eq!(primary.len(), 2);
        assert_eq!(primary[0], AxisScale::Linear);
        assert_eq!(primary[1], AxisScale::Log);
    }

    #[test]
    fn axis_bound_steppers_adjust_min_and_max_independently() {
        let mut range = (0.0, 10.0);
        // Stepping the minimum does not move the maximum.
        adjust_axis_bound(&mut range, &AxisScale::Linear, true, 0.8);
        assert_eq!(range, (0.0, 10.0));
        adjust_axis_bound(&mut range, &AxisScale::Linear, true, 1.25);
        assert!((range.0 - 0.0).abs() < 1e-9);
        // Stepping the maximum does not move the minimum.
        adjust_axis_bound(&mut range, &AxisScale::Linear, false, 0.8);
        assert!((range.1 - 8.0).abs() < 1e-9);
        adjust_axis_bound(&mut range, &AxisScale::Linear, false, 1.25);
        assert!((range.1 - 10.0).abs() < 1e-9);

        // The bounds must never cross.
        let mut crossed = (2.0, 3.0);
        adjust_axis_bound(&mut crossed, &AxisScale::Linear, true, 2.0);
        assert_eq!(crossed, (2.0, 3.0));

        // Non-finite input is ignored.
        let mut nan_range = (f64::NAN, 1.0);
        adjust_axis_bound(&mut nan_range, &AxisScale::Linear, false, 0.8);
        assert!(nan_range.0.is_nan());
        assert_eq!(nan_range.1, 1.0);
    }

    #[test]
    fn drive_interact_places_a_quadrant_gate_on_click_without_a_drag() {
        let mut interact = Interact::default();
        let inter = PlotInteraction {
            clicked: Some([5.0, 5.0]),
            ..Default::default()
        };
        let outcome = drive_interact(
            &mut interact,
            &inter,
            &[],
            None,
            NewGateType::Quadrant,
            0.02,
        );
        assert!(matches!(outcome, Some(InteractOutcome::Click([5.0, 5.0]))));

        // A click with a non-placeable gate type does not create a gate.
        let outcome = drive_interact(&mut interact, &inter, &[], None, NewGateType::Rect, 0.02);
        assert!(outcome.is_none());
    }

    #[test]
    fn gate_at_finds_any_gate_body_and_prefers_the_selection() {
        let rect = |min: f64, max: f64| GateKind::Rect {
            x_min: min,
            x_max: max,
            y_min: min,
            y_max: max,
            angle: 0.0,
        };
        let gate = |name: &str, kind: GateKind| GateDef {
            name: name.into(),
            x_idx: 0,
            y_idx: 1,
            parent: None,
            kind,
            color: egui::Color32::WHITE,
            owner: None,
            apply_to_all: true,
        };
        let gates = vec![
            gate("wide", rect(-1.0, 11.0)),
            gate("small", rect(4.0, 6.0)),
            gate(
                "ellipse",
                GateKind::Ellipse {
                    x_min: 20.0,
                    x_max: 30.0,
                    y_min: 20.0,
                    y_max: 30.0,
                    angle: 0.0,
                },
            ),
        ];

        // Without a selection the topmost (last-drawn) body wins.
        assert_eq!(gate_at(&gates, [5.0, 5.0], None), Some(1));
        // Clicking the selected gate keeps it selected even when another body
        // overlaps it.
        assert_eq!(gate_at(&gates, [5.0, 5.0], Some(0)), Some(0));
        assert_eq!(gate_at(&gates, [5.0, 5.0], Some(1)), Some(1));
        // Empty space is not a gate body.
        assert_eq!(gate_at(&gates, [50.0, 50.0], None), None);
        // Ellipse bodies are hit-tested as ellipses, not bounding boxes.
        assert_eq!(gate_at(&gates, [25.0, 25.0], None), Some(2));
        assert_eq!(gate_at(&gates, [20.1, 20.1], None), None);
    }

    #[test]
    fn gatingml_document_from_gates_exports_rect_and_skips_bilinear() {
        let rect_gate = GateDef {
            name: "Live".into(),
            x_idx: 0,
            y_idx: 1,
            parent: None,
            kind: GateKind::Rect {
                x_min: 0.0,
                x_max: 10.0,
                y_min: 0.0,
                y_max: 10.0,
                angle: 0.0,
            },
            color: egui::Color32::WHITE,
            owner: None,
            apply_to_all: true,
        };
        let bilinear_gate = GateDef {
            name: "Line".into(),
            x_idx: 0,
            y_idx: 1,
            parent: None,
            kind: GateKind::Bilinear {
                x1: 0.0,
                y1: 0.0,
                x2: 10.0,
                y2: 10.0,
                below: true,
            },
            color: egui::Color32::WHITE,
            owner: None,
            apply_to_all: true,
        };
        let gates = vec![rect_gate, bilinear_gate];
        let param_names = vec!["P0".to_string(), "P1".to_string()];
        let doc = gatingml_document_from_gates(
            crate::gatingml::GatingMlVersion::V2_0,
            &gates,
            &param_names,
            None,
        );

        // The bilinear line gate has no Gating-ML representation and is skipped
        // with a warning rather than silently dropped.
        assert_eq!(doc.gates.len(), 1);
        assert_eq!(doc.gates[0].name, "Live");
        assert_eq!(doc.gates[0].id, "gate-1");
        assert_eq!(doc.gates[0].parent_id, None);
        assert!(matches!(
            doc.gates[0].kind,
            crate::gatingml::GatingMlGateKind::Rectangle { .. }
        ));
        assert_eq!(doc.warnings.len(), 1);
        assert!(doc.warnings[0].contains("no Gating-ML representation"));

        // A quadrant exports with its split positions.
        let quad_gate = GateDef {
            name: "Quad".into(),
            x_idx: 0,
            y_idx: 1,
            parent: None,
            kind: GateKind::Quadrant {
                x: 5.0,
                y: 5.0,
                quad: Quadrant::UpperRight,
            },
            color: egui::Color32::WHITE,
            owner: None,
            apply_to_all: true,
        };
        let doc = gatingml_document_from_gates(
            crate::gatingml::GatingMlVersion::V2_0,
            std::slice::from_ref(&quad_gate),
            &param_names,
            None,
        );
        assert_eq!(doc.gates.len(), 1);
        assert!(matches!(
            doc.gates[0].kind,
            crate::gatingml::GatingMlGateKind::Quadrant { .. }
        ));
        assert_eq!(doc.warnings.len(), 0);
    }

    #[test]
    fn gatingml_gate_to_kind_maps_rectangle_and_quadrant() {
        let rect = crate::gatingml::GatingMlGate {
            id: "gate-1".into(),
            parent_id: None,
            name: "Live".into(),
            kind: crate::gatingml::GatingMlGateKind::Rectangle {
                x_param: "P0".into(),
                y_param: "P1".into(),
                x_min: 0.0,
                x_max: 10.0,
                y_min: 0.0,
                y_max: 10.0,
            },
        };
        let (kind, x_idx, y_idx) =
            gatingml_gate_to_kind(&rect, &["P0".to_string(), "P1".to_string()])
                .expect("rectangle gates should map");
        assert!(matches!(kind, GateKind::Rect { .. }));
        assert_eq!((x_idx, y_idx), (0, 1));

        let quad = crate::gatingml::GatingMlGate {
            id: "gate-2".into(),
            parent_id: Some("gate-1".into()),
            name: "Quad".into(),
            kind: crate::gatingml::GatingMlGateKind::Quadrant {
                x_param: "P0".into(),
                y_param: "P1".into(),
                x_split: 5.0,
                y_split: 5.0,
                quadrant: "UR".into(),
            },
        };
        let (kind, x_idx, y_idx) =
            gatingml_gate_to_kind(&quad, &["P0".to_string(), "P1".to_string()])
                .expect("quadrant gates should map");
        assert!(matches!(kind, GateKind::Quadrant { .. }));
        assert_eq!((x_idx, y_idx), (0, 1));

        // Unknown parameter names resolve to None (unmappable gate).
        assert!(gatingml_gate_to_kind(&quad, &["P9".to_string()]).is_none());
    }

    #[test]
    fn mask_gate_population_selects_indexed_events() {
        let dir = std::path::Path::new("/tmp/opencode");
        std::fs::create_dir_all(dir).unwrap();
        let fcs_path = dir.join("mask_gate.fcs");
        write_fcs(&fcs_path, 4, 2);

        let mut app = FlowersApp::default();
        app.load_file(&fcs_path);
        let index = app.push_gate(
            "Mask".into(),
            GateKind::Mask {
                indices: vec![1, 3],
            },
            None,
            0,
            0,
        );
        app.recompute_populations();
        assert_eq!(app.gates[index].kind.summary(), "mask (2 events)");

        let pops = compute_gate_populations(
            &app.samples[0].fcs.data,
            &app.gates,
            Some(0),
            app.samples[0].apply_gates,
        );
        assert_eq!(pops[index].len(), 2);
        assert!(pops[index].contains(&1));
        assert!(pops[index].contains(&3));
    }

    #[test]
    fn import_mask_adds_a_mask_gate_with_the_selected_events() {
        let dir = std::path::Path::new("/tmp/opencode");
        std::fs::create_dir_all(dir).unwrap();
        let fcs_path = dir.join("mask_import.fcs");
        write_fcs(&fcs_path, 4, 2);
        let mask_path = dir.join("mask_import.txt");
        std::fs::write(&mask_path, "1\n3\n").unwrap();

        let mut app = FlowersApp::default();
        app.load_file(&fcs_path);
        app.import_mask(&mask_path).unwrap();
        assert_eq!(app.gates.len(), 1);
        assert!(matches!(app.gates[0].kind, GateKind::Mask { .. }));
        assert_eq!(app.gates[0].name, "Mask");

        let pops = compute_gate_populations(
            &app.samples[0].fcs.data,
            &app.gates,
            Some(0),
            app.samples[0].apply_gates,
        );
        assert_eq!(pops[0].len(), 2);
    }

    #[test]
    fn compensation_can_be_applied_and_removed_without_losing_raw_data() {
        let dir = std::path::Path::new("/tmp/opencode");
        std::fs::create_dir_all(dir).unwrap();
        let fcs_path = dir.join("compensation_apply.fcs");
        write_fcs(&fcs_path, 4, 2);

        let mut app = FlowersApp::default();
        app.load_file(&fcs_path);
        assert_eq!(app.samples.len(), 1);
        let raw = app.samples[0].raw_data.clone();
        assert_eq!(app.samples[0].fcs.data, raw);

        let matrix = CompensationMatrix::new(
            vec!["P1".into(), "P2".into()],
            vec![vec![1.0, 0.5], vec![0.0, 1.0]],
        )
        .unwrap();
        app.comp_draft = Some(matrix.clone());
        app.apply_compensation(false);

        // P2 = 0.5 * P1 + P2 for each event, and the raw data is retained.
        assert_eq!(app.samples[0].compensation.as_ref(), Some(&matrix));
        assert_eq!(app.samples[0].raw_data, raw);
        assert_eq!(app.samples[0].fcs.data[[3, 0]], 3.0);
        assert_eq!(app.samples[0].fcs.data[[3, 1]], 4.5);
        assert!(app.dirty);

        app.clear_compensation(false);
        assert!(app.samples[0].compensation.is_none());
        assert_eq!(app.samples[0].fcs.data, raw);
    }

    #[test]
    fn compensation_node_visibility_toggles_and_reports_status() {
        let mut app = FlowersApp::default();
        assert!(app.comp_node_visible);
        assert_eq!(
            app.compensation_node_status(),
            "Compensation node idle (no matrix applied)."
        );
        app.toggle_compensation_node();
        assert!(!app.comp_node_visible);
        assert_eq!(app.compensation_node_status(), "Compensation node hidden.");
        app.toggle_compensation_node();
        assert!(app.comp_node_visible);
        assert_eq!(
            app.compensation_node_status(),
            "Compensation node idle (no matrix applied)."
        );
    }

    #[test]
    fn magnetic_snapping_pulls_drawing_points_toward_events() {
        let dir = std::path::Path::new("/tmp/opencode");
        std::fs::create_dir_all(dir).unwrap();
        let fcs_path = dir.join("magnetic_snap.fcs");
        // Events lie on the diagonal: (0,0), (1,1), ..., (11,11).
        write_fcs(&fcs_path, 12, 2);

        let mut app = FlowersApp::default();
        app.load_file(&fcs_path);
        app.magnetic_radius_frac = 0.5;
        app.magnetic_strength = 1.0;

        // Disabled snapping leaves the pointer exactly where it was.
        app.magnetic_snap = false;
        let point = [4.2, 1.0];
        assert_eq!(app.snap_drawing_point(point, 0, 1, 10.0), point);

        // Enabled snapping pulls the off-diagonal point toward the events.
        app.magnetic_snap = true;
        let snapped = app.snap_drawing_point(point, 0, 1, 10.0);
        assert!(
            snapped[1] > point[1],
            "expected y to move toward the diagonal: {snapped:?}"
        );
        assert!((snapped[1] - snapped[0]).abs() < (point[1] - point[0]).abs());

        // Zero strength is the same as disabled.
        app.magnetic_strength = 0.0;
        assert_eq!(app.snap_drawing_point(point, 0, 1, 10.0), point);

        // A gate created while snapping is enabled is still valid.
        app.magnetic_strength = 1.0;
        app.x_param = Some(0);
        app.y_param = Some(1);
        let created = app.create_gates([4.2, 1.0], [8.2, 5.0], 0, 1, None, NewGateType::Rect, 10.0);
        assert_eq!(created.len(), 1);
        assert!(matches!(app.gates[created[0]].kind, GateKind::Rect { .. }));
    }

    #[test]
    fn autorange_rule_overrides_autofit_bounds() {
        let dir = std::path::Path::new("/tmp/opencode");
        std::fs::create_dir_all(dir).unwrap();
        let fcs_path = dir.join("autorange.fcs");
        write_fcs(&fcs_path, 12, 2);

        let mut app = FlowersApp::default();
        app.load_file(&fcs_path);
        app.x_param = Some(0);
        app.y_param = Some(1);
        app.plot_points = (0..12).map(|i| [i as f64, i as f64]).collect();

        // Without a rule the auto-fit uses the data-driven bounds.
        let values: Vec<f64> = app.plot_points.iter().map(|p| p[0]).collect();
        let auto = app
            .autorange_bounds(&values, 0)
            .unwrap_or_else(|| bounds_for(&values, &app.x_scale));
        assert!(auto.0 < auto.1, "auto bounds: {auto:?}");

        // A name-matched FSC rule with explicit min/max overrides the auto-fit.
        app.autorange_rules.push(AutorangeRule {
            id: "ar1".to_string(),
            name: "FSC autorange".to_string(),
            priority: 0,
            enabled: true,
            matcher: crate::transform::AutorangeMatcher::Name("P1*".to_string()),
            limit: crate::transform::AutorangeLimit::MinMax {
                min: 0.0,
                max: 10.0,
            },
        });
        assert_eq!(app.autorange_bounds(&values, 0), Some((0.0, 10.0)));

        // Disabled rules are ignored.
        app.autorange_rules[0].enabled = false;
        assert!(app.autorange_bounds(&values, 0).is_none());
    }

    #[test]
    fn statistics_panel_set_crud_and_open_close() {
        let dir = std::path::Path::new("/tmp/opencode");
        std::fs::create_dir_all(dir).unwrap();
        let fcs_path = dir.join("stats_panel.fcs");
        write_fcs(&fcs_path, 12, 2);

        let mut app = FlowersApp::default();
        app.load_file(&fcs_path);

        // Panel starts closed and toggling through the public methods works.
        assert!(!app.stats_panel_open);
        app.open_stats_panel();
        assert!(app.stats_panel_open);
        app.stats_panel_open = false;
        assert!(!app.stats_panel_open);

        // With an empty registry the add/copy actions are no-ops.
        app.stats_panel_open = true;
        app.stats_panel_add_set(0);
        app.stats_panel_copy_set(0);
        assert!(app.statistic_sets.is_empty());
        app.stats_panel_open = false;
        app.stats_panel_remove_set(0);

        // Add a set, copy it (two entries), then remove one.
        app.statistic_sets
            .push(crate::statistics::StatisticSet::new(
                "Main",
                vec![crate::statistics::StatSpec {
                    parameter: Some("P1".to_string()),
                    statistic: crate::statistics::Statistic::Mean,
                    expected: None,
                    heat: None,
                }],
            ));
        app.stats_panel_open = true;
        app.stats_panel_add_set(0);
        assert_eq!(app.statistic_sets.len(), 2);
        app.stats_panel_remove_set(0);
        assert_eq!(app.statistic_sets.len(), 1);
    }

    #[test]
    fn statistics_panel_rows_are_empty_without_a_population() {
        let dir = std::path::Path::new("/tmp/opencode");
        std::fs::create_dir_all(dir).unwrap();
        let fcs_path = dir.join("stats_panel2.fcs");
        write_fcs(&fcs_path, 12, 2);

        let mut app = FlowersApp::default();
        app.load_file(&fcs_path);
        // No population selected and no cached events yet.
        assert!(app.stats_panel_rows().is_empty());
    }

    #[test]
    fn gate_copy_paste_and_duplicate_preserve_parent() {
        let mut app = FlowersApp::default();
        let parent = app.push_gate(
            "Parent".into(),
            GateKind::Rect {
                x_min: 0.0,
                x_max: 4.0,
                y_min: 0.0,
                y_max: 4.0,
                angle: 0.0,
            },
            None,
            0,
            1,
        );
        let child = app.push_gate(
            "Child".into(),
            GateKind::Ellipse {
                x_min: 1.0,
                x_max: 3.0,
                y_min: 1.0,
                y_max: 3.0,
                angle: 0.0,
            },
            Some(parent),
            0,
            1,
        );

        // Copy serializes the geometry; paste recreates it as a sibling.
        let json = app.copy_gate(child).unwrap();
        assert!(json.contains("ellipse"), "clipboard JSON: {json}");
        let pasted = app.paste_gate().unwrap();
        assert_eq!(app.gates[pasted].parent, Some(parent));
        assert_eq!(app.gates[pasted].name, "Child");
        assert_eq!(app.gates[pasted].kind, app.gates[child].kind);

        // Duplicate keeps the parent and gets a unique sibling name.
        let duplicate = app.duplicate_gate(child).unwrap();
        assert_eq!(app.gates[duplicate].parent, Some(parent));
        assert_eq!(app.gates[duplicate].name, "Child copy");
        assert_eq!(app.gates[duplicate].owner, app.gates[child].owner);
        assert_eq!(app.gates[duplicate].kind, app.gates[child].kind);
        let second = app.duplicate_gate(child).unwrap();
        assert_eq!(app.gates[second].name, "Child copy 2");

        // An empty or malformed clipboard cannot paste.
        app.gate_clipboard = None;
        assert!(app.paste_gate().is_none());
        app.gate_clipboard = Some("not json".to_string());
        assert!(app.paste_gate().is_none());
    }

    #[test]
    fn sync_named_gates_updates_only_matching_parameter_pairs() {
        let rect = |x_min: f64, x_max: f64| GateKind::Rect {
            x_min,
            x_max,
            y_min: 0.0,
            y_max: 4.0,
            angle: 0.0,
        };
        let mut app = FlowersApp::default();
        let source = app.push_gate("CD4".into(), rect(0.0, 4.0), None, 0, 1);
        let same_name = app.push_gate("CD4".into(), rect(1.0, 9.0), None, 0, 1);
        let other_pair = app.push_gate("CD4".into(), rect(1.0, 9.0), None, 1, 2);
        let other_name = app.push_gate("CD8".into(), rect(1.0, 9.0), None, 0, 1);

        let changed = app.sync_named_gates(source);
        assert_eq!(changed, 1);
        assert_eq!(app.gates[same_name].kind, app.gates[source].kind);
        assert_ne!(app.gates[other_pair].kind, app.gates[source].kind);
        assert_ne!(app.gates[other_name].kind, app.gates[source].kind);

        // Synchronising again is a no-op.
        assert_eq!(app.sync_named_gates(source), 0);
    }

    #[test]
    fn gate_labels_and_backgating_follow_the_population_tree() {
        let mut app = FlowersApp::default();
        let parent = app.push_gate(
            "Lymph".into(),
            GateKind::Rect {
                x_min: 0.0,
                x_max: 8.0,
                y_min: 0.0,
                y_max: 8.0,
                angle: 0.0,
            },
            None,
            0,
            1,
        );
        let child = app.push_gate(
            "CD4+".into(),
            GateKind::Rect {
                x_min: 1.0,
                x_max: 4.0,
                y_min: 1.0,
                y_max: 4.0,
                angle: 0.0,
            },
            Some(parent),
            0,
            1,
        );
        assert!(child > parent);
        // Simulate cached populations: 10 events in the parent, 4 in the child.
        app.pop_indices = vec![(0..10).collect(), (0..4).collect()];

        // Labels on by default: name and count, no ancestry unless enabled.
        let visuals = app.gate_visuals_for(0, 1, (0.0, 10.0), (0.0, 10.0), Some(child));
        assert_eq!(visuals.len(), 2);
        let child_visual = &visuals[child];
        let (text, position) = child_visual.label.as_ref().expect("child label");
        assert!(text.contains("CD4+"));
        assert!(text.contains("n=4"));
        assert!(!text.contains("Lymph"));
        assert!(position[0].is_finite() && position[1].is_finite());
        assert!(!child_visual.backgate);

        // Enabling ancestry and backgating adds the path and marks the parent.
        app.gate_labels.ancestry = true;
        app.gate_labels.backgating = true;
        let visuals = app.gate_visuals_for(0, 1, (0.0, 10.0), (0.0, 10.0), Some(child));
        let text = &visuals[child].label.as_ref().unwrap().0;
        assert!(text.contains("Lymph / CD4+"));
        assert!(visuals[parent].backgate);
        assert!(!visuals[child].backgate);

        // Hiding labels removes them all.
        app.gate_labels.show = false;
        let visuals = app.gate_visuals_for(0, 1, (0.0, 10.0), (0.0, 10.0), Some(child));
        assert!(visuals.iter().all(|g| g.label.is_none()));
    }

    #[test]
    fn convert_gate_changes_kind_and_geometry() {
        let mut app = FlowersApp::default();
        app.push_gate(
            "G".into(),
            GateKind::Rect {
                x_min: 0.0,
                x_max: 4.0,
                y_min: 2.0,
                y_max: 6.0,
                angle: 0.0,
            },
            None,
            0,
            1,
        );

        app.convert_gate(0, crate::gating::GateShape::Ellipse);
        match &app.gates[0].kind {
            GateKind::Ellipse {
                x_min,
                x_max,
                y_min,
                y_max,
                ..
            } => {
                assert!((x_min - 0.0).abs() < 1e-6);
                assert!((x_max - 4.0).abs() < 1e-6);
                assert!((y_min - 2.0).abs() < 1e-6);
                assert!((y_max - 6.0).abs() < 1e-6);
            }
            other => panic!("expected ellipse, got {other:?}"),
        }

        app.convert_gate(0, crate::gating::GateShape::Polygon);
        match &app.gates[0].kind {
            GateKind::Polygon { vertices } => assert_eq!(vertices.len(), 64),
            other => panic!("expected polygon, got {other:?}"),
        }

        // Quadrant gates cannot be converted and stay unchanged.
        app.push_gate(
            "Q".into(),
            GateKind::Quadrant {
                x: 5.0,
                y: 5.0,
                quad: Quadrant::UpperLeft,
            },
            None,
            0,
            1,
        );
        app.convert_gate(1, crate::gating::GateShape::Rectangle);
        assert!(matches!(app.gates[1].kind, GateKind::Quadrant { .. }));
    }

    #[test]
    fn undo_redo_restores_gate_edits() {
        let rect = || GateKind::Rect {
            x_min: 0.0,
            x_max: 1.0,
            y_min: 0.0,
            y_max: 1.0,
            angle: 0.0,
        };
        let mut app = FlowersApp::default();
        // Seed the empty starting state (as the first frame would).
        app.record_history(false);
        assert!(!app.can_undo());
        assert!(!app.can_redo());

        app.push_gate("G1".into(), rect(), None, 0, 1);
        app.record_history(false);
        assert_eq!(app.gates.len(), 1);
        assert!(app.can_undo());

        app.push_gate("G2".into(), rect(), None, 0, 1);
        app.record_history(false);
        assert_eq!(app.gates.len(), 2);

        app.undo();
        assert_eq!(app.gates.len(), 1);
        assert!(app.can_redo());
        app.undo();
        assert_eq!(app.gates.len(), 0);
        assert!(!app.can_undo());

        app.redo();
        assert_eq!(app.gates.len(), 1);
        app.redo();
        assert_eq!(app.gates.len(), 2);
        assert!(!app.can_redo());
    }

    #[test]
    fn quadrant_move_keeps_all_four_wedges_in_sync() {
        let mut app = FlowersApp::default();
        for (i, quad) in [
            Quadrant::UpperLeft,
            Quadrant::UpperRight,
            Quadrant::LowerLeft,
            Quadrant::LowerRight,
        ]
        .into_iter()
        .enumerate()
        {
            let kind = GateKind::Quadrant {
                x: 5.0,
                y: 5.0,
                quad,
            };
            app.push_gate(format!("Q{}", i), kind, None, 0, 1);
        }
        assert_eq!(app.gates.len(), 4);
        for gate in &app.gates {
            assert_eq!(gate.kind.quadrant_split(), Some([5.0, 5.0]));
        }

        // Move the first wedge (as a drag on its body / centre handle would).
        let old = app.gates[0].kind.clone();
        app.gates[0].kind = GateKind::Quadrant {
            x: 7.0,
            y: 5.0,
            quad: Quadrant::UpperLeft,
        };
        let new_kind = app.gates[0].kind.clone();
        app.commit_gate_edit(0, &old, &new_kind);

        // The shared split must move for the whole group of four wedges.
        assert_eq!(app.gates[0].kind.quadrant_split(), Some([7.0, 5.0]));
        for gate in &app.gates[1..] {
            assert_eq!(gate.kind.quadrant_split(), Some([7.0, 5.0]));
        }
    }

    #[test]
    fn undo_is_not_duplicated_when_nothing_changes() {
        let mut app = FlowersApp::default();
        app.record_history(false);
        // No edit: repeated frames must not grow the history.
        app.record_history(false);
        app.record_history(false);
        assert_eq!(app.history.len(), 1);
        assert!(!app.can_undo());
    }

    #[test]
    fn selecting_a_gate_does_not_create_history_entry() {
        let mut app = FlowersApp::default();
        app.record_history(false);
        app.push_gate(
            "G1".into(),
            GateKind::Rect {
                x_min: 0.0,
                x_max: 1.0,
                y_min: 0.0,
                y_max: 1.0,
                angle: 0.0,
            },
            None,
            0,
            1,
        );
        app.record_history(false);
        assert_eq!(app.history.len(), 2);
        // Selecting a gate is navigation, not an edit.
        app.selected_gate = Some(0);
        app.record_history(false);
        assert_eq!(app.history.len(), 2);
        assert!(!app.can_redo());
    }

    /// Writes a minimal, valid FCS 3.0 file (see `workspace::tests`).
    fn write_fcs(path: &std::path::Path, events: usize, params: usize) {
        use crate::fcs::FCS_HEADER_LEN;
        let mut text = Vec::new();
        text.push(b'/');
        text.extend_from_slice(format!("$TOT/{events}/").as_bytes());
        text.extend_from_slice(format!("$PAR/{params}/").as_bytes());
        for n in 1..=params {
            text.extend_from_slice(format!("$P{n}N/P{n}/").as_bytes());
            text.extend_from_slice(format!("$P{n}R/4.0/").as_bytes());
        }
        text.extend_from_slice(b"$BYTEORD/1,2,3,4/");
        let text_start = FCS_HEADER_LEN;
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
        buf.extend_from_slice(b"FCS3.0    ");
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
        std::fs::write(path, buf).unwrap();
    }

    #[test]
    fn loaded_plot_card_is_not_clipped_by_the_window_bottom() {
        let dir = std::path::Path::new("/tmp/opencode");
        std::fs::create_dir_all(dir).unwrap();
        let fcs_path = dir.join("plot_card_clipping.fcs");
        write_fcs(&fcs_path, 4, 2);

        let mut app = FlowersApp::default();
        app.load_file(&fcs_path);

        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
        let raw = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        // Run a few frames so the resizable bottom panel settles on its
        // content height before checking the central plot card.
        for _ in 0..3 {
            let _ = ctx.run(raw.clone(), |ctx| app.draw(ctx));
        }

        let (content, clip) = ctx
            .data(|d| {
                d.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new("central_plot_card_rects"))
            })
            .expect("plot card layout should be recorded");
        assert!(
            content.min.y >= clip.min.y - 0.5 && content.max.y <= clip.max.y + 0.5,
            "plot card content must fit inside the central panel without being \
             clipped at the bottom (content={content:?}, clip={clip:?})"
        );
        assert!(
            content.min.x >= clip.min.x - 0.5 && content.max.x <= clip.max.x + 0.5,
            "plot card content must fit inside the central panel horizontally \
             (content={content:?}, clip={clip:?})"
        );
    }

    #[test]
    fn save_and_load_workspace_round_trips() {
        let dir = std::path::Path::new("/tmp/opencode");
        std::fs::create_dir_all(dir).unwrap();
        let fcs_path = dir.join("ws_roundtrip.fcs");
        write_fcs(&fcs_path, 4, 2);

        let mut app = FlowersApp::default();
        app.load_file(&fcs_path);
        assert_eq!(app.samples.len(), 1);
        app.push_gate(
            "G1".into(),
            GateKind::Rect {
                x_min: 0.0,
                x_max: 1.0,
                y_min: 0.5,
                y_max: 2.0,
                angle: 0.25,
            },
            None,
            0,
            1,
        );
        app.selected_gate = Some(0);
        app.view_mode = ViewMode::Contour;
        app.contour_levels = 6;
        app.density_map = crate::plot::DensityMap::Zebra;
        app.overlay_populations = true;
        app.transpose = true;
        app.plot_smoothing = 0.5;
        app.plot_resolution = 256;
        app.outlier_clip_percent = 5.0;
        app.large_dots = true;
        app.show_grid = true;
        app.show_medians = true;
        app.show_event_numbers = true;
        app.show_names_frequencies = true;
        app.show_axis_histograms = true;
        app.tick_notation = crate::plot::TickNotation::Scientific;
        app.show_axis_labels = false;
        app.show_ticks = false;
        app.show_numbers = false;
        app.layout_columns = 3;
        app.add_layout_current();
        assert_eq!(app.layout_panels.len(), 1);

        let matrix = CompensationMatrix::new(
            vec!["P1".into(), "P2".into()],
            vec![vec![1.0, 0.25], vec![0.0, 1.0]],
        )
        .unwrap();
        app.comp_draft = Some(matrix.clone());
        app.apply_compensation(false);
        assert_eq!(app.samples[0].compensation.as_ref(), Some(&matrix));

        // Set the manual ranges after compensation: applying a matrix refreshes
        // the plot points and may auto-fit the axes.
        app.x_range = (1.0, 3.0);
        app.y_range = (2.0, 4.0);

        let wsp = dir.join("ws_roundtrip.wsp");
        app.save_workspace(&wsp);
        assert!(wsp.exists());

        let mut app2 = FlowersApp::default();
        app2.load_workspace(&wsp);
        assert_eq!(app2.samples.len(), 1);
        assert_eq!(app2.samples[0].id, "ws_roundtrip.fcs");
        assert!(app2.samples[0].path.ends_with("ws_roundtrip.fcs"));
        assert_eq!(app2.gates.len(), 1);
        assert_eq!(app2.gates[0].name, "G1");
        assert_eq!(
            app2.gates[0].kind,
            GateKind::Rect {
                x_min: 0.0,
                x_max: 1.0,
                y_min: 0.5,
                y_max: 2.0,
                angle: 0.25,
            }
        );
        assert_eq!(app2.selected_gate, Some(0));
        assert_eq!(app2.view_mode, ViewMode::Contour);
        assert_eq!(app2.contour_levels, 6);
        assert_eq!(app2.density_map, crate::plot::DensityMap::Zebra);
        assert!(app2.overlay_populations);
        assert!(app2.transpose);
        assert_eq!(app2.plot_smoothing, 0.5);
        assert_eq!(app2.plot_resolution, 256);
        assert_eq!(app2.outlier_clip_percent, 5.0);
        assert!(app2.large_dots);
        assert!(app2.show_grid);
        assert!(app2.show_medians);
        assert!(app2.show_event_numbers);
        assert!(app2.show_names_frequencies);
        assert!(app2.show_axis_histograms);
        assert_eq!(app2.tick_notation, crate::plot::TickNotation::Scientific);
        assert!(!app2.show_axis_labels);
        assert!(!app2.show_ticks);
        assert!(!app2.show_numbers);
        assert_eq!(app2.x_range, (1.0, 3.0));
        assert_eq!(app2.y_range, (2.0, 4.0));
        assert_eq!(app2.layout_columns, 3);
        assert_eq!(app2.layout_panels.len(), 1);
        assert_eq!(app2.layout_panels[0].sample, 0);
        // Compensation is saved with the workspace and reapplied to the raw
        // data when the workspace is opened again.
        assert_eq!(app2.samples[0].compensation.as_ref(), Some(&matrix));
        assert_ne!(app2.samples[0].fcs.data, app2.samples[0].raw_data);
    }

    #[test]
    fn pseudocolor_export_uses_the_selected_density_map() {
        let mut fcs = fcs_with(&["P1", "P2"]);
        fcs.event_count = 4;
        fcs.data = array![[1.0, 1.0], [2.0, 2.0], [3.0, 3.0], [4.0, 4.0]];
        let mut app = FlowersApp::default();
        app.samples.push(SampleEntry {
            id: "density.fcs".to_string(),
            path: std::path::PathBuf::from("/tmp/opencode/density.fcs"),
            raw_data: fcs.data.clone(),
            fcs,
            compensation: None,
            apply_gates: true,
        });
        app.current_sample = Some(0);
        app.x_param = Some(0);
        app.y_param = Some(1);
        app.x_range = (0.0, 5.0);
        app.y_range = (0.0, 5.0);

        // Dot mode has no density legend.
        let (dot_svg, _, _) = app.current_plot_svg().unwrap();
        assert!(!dot_svg.contains("linearGradient"));

        // Pseudocolour mode embeds the map legend and zebra-coloured points.
        app.view_mode = ViewMode::Pseudocolor;
        app.density_map = crate::plot::DensityMap::Zebra;
        let (svg, _, _) = app.current_plot_svg().unwrap();
        assert!(svg.contains("linearGradient"));
        assert!(svg.contains("#d9d9d9") || svg.contains("#262626"));

        // Unknown persisted ids fall back to the rainbow map.
        let mut project = app.to_project();
        project.view.density_map = "not-a-map".to_string();
        let dir = std::path::Path::new("/tmp/opencode");
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join("density_fallback.wsp");
        let json = serde_json::to_string_pretty(&project).unwrap();
        std::fs::write(&path, json).unwrap();
        app.load_workspace(&path);
        assert_eq!(app.density_map, crate::plot::DensityMap::Rainbow);
    }

    /// Builds an app with one synthetic 3-parameter sample and no gates.
    fn app_with_synthetic_sample() -> FlowersApp {
        let names = ["P1", "P2", "P3"];
        let mut fcs = fcs_with(&names);
        fcs.event_count = 4;
        fcs.data = array![
            [1.0, 4.0, 7.0],
            [2.0, 5.0, 8.0],
            [3.0, 6.0, 9.0],
            [4.0, 7.0, 10.0]
        ];
        let mut app = FlowersApp::default();
        app.samples.push(SampleEntry {
            id: "multi.fcs".to_string(),
            path: std::path::PathBuf::from("/tmp/opencode/multi.fcs"),
            raw_data: fcs.data.clone(),
            fcs,
            compensation: None,
            apply_gates: true,
        });
        app.current_sample = Some(0);
        app.x_param = Some(0);
        app.y_param = Some(1);
        app
    }

    #[test]
    fn data_overlays_follow_switch_selection_and_parameter_pair() {
        let mut app = app_with_synthetic_sample();
        let all = GateKind::Rect {
            x_min: 0.0,
            x_max: 20.0,
            y_min: 0.0,
            y_max: 20.0,
            angle: 0.0,
        };
        app.push_gate("G1".into(), all.clone(), None, 0, 1);
        app.push_gate("G2".into(), all.clone(), None, 0, 1);
        // A gate on another parameter pair must not appear as an overlay.
        app.push_gate("Other".into(), all, None, 0, 2);
        app.recompute_populations();
        app.selected_gate = Some(0);

        app.overlay_populations = false;
        assert!(app.data_overlays_for_plot().is_empty());

        app.overlay_populations = true;
        let overlays = app.data_overlays_for_plot();
        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].label, "G2");
        assert_eq!(overlays[0].kind, crate::plot::OverlayKind::Dots);
        assert_eq!(overlays[0].points.len(), 4);
    }

    #[test]
    fn multigraph_sheet_renders_every_parameter() {
        let app = app_with_synthetic_sample();
        let (svg, w, h) = app
            .multigraph_svg_for(crate::plot::MultigraphMode::AllByX, 3)
            .unwrap();
        assert_eq!(w, 3 * 280 + 4 * 8);
        assert_eq!(h, 220 + 2 * 8);
        assert_eq!(svg.matches("<g transform=\"translate(").count(), 3);

        let (nxn, _, _) = app
            .multigraph_svg_for(crate::plot::MultigraphMode::NxN, 2)
            .unwrap();
        assert_eq!(nxn.matches("<g transform=\"translate(").count(), 6);
    }

    #[test]
    fn backgate_residual_events_exclude_the_selected_gate() {
        let mut app = app_with_synthetic_sample();
        app.push_gate(
            "G1".into(),
            GateKind::Rect {
                x_min: 1.0,
                x_max: 2.0,
                y_min: 0.0,
                y_max: 100.0,
                angle: 0.0,
            },
            None,
            0,
            1,
        );
        app.recompute_populations();
        app.selected_gate = Some(0);
        app.overlay_populations = false;
        app.gate_labels.backgate_events = true;

        let overlays = app.data_overlays_for_plot();
        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].label, "Excluded by G1");
        // Two of the four events are inside the gate; two are residual.
        assert_eq!(overlays[0].points.len(), 2);
        assert!(overlays[0].points.iter().all(|p| p[0] > 2.0));

        // Turning the switch off removes the residual layer.
        app.gate_labels.backgate_events = false;
        assert!(app.data_overlays_for_plot().is_empty());
    }

    #[test]
    fn graph_controls_transpose_points_gates_and_axes() {
        let mut app = app_with_synthetic_sample();
        app.update_plot_points();
        app.push_gate(
            "G".into(),
            GateKind::Rect {
                x_min: 1.0,
                x_max: 2.0,
                y_min: 1.0,
                y_max: 3.0,
                angle: 0.0,
            },
            None,
            0,
            1,
        );
        app.recompute_populations();
        let before = app.main_plot_cfg();
        let original_fill = before.gates[0].fill.clone();
        assert_eq!(before.points[0], [1.0, 4.0]);

        app.x_scale = AxisScale::Linear;
        app.y_scale = AxisScale::Arcsinh { cofactor: 150.0 };
        app.x_range = (1.0, 2.0);
        app.y_range = (3.0, 4.0);
        app.transpose = true;
        let cfg = app.main_plot_cfg();

        // Axes, ranges, scales and points are swapped for presentation.
        assert_eq!(cfg.x_range, (3.0, 4.0));
        assert_eq!(cfg.y_range, (1.0, 2.0));
        assert!(matches!(cfg.x_scale, AxisScale::Arcsinh { .. }));
        assert_eq!(cfg.y_scale, AxisScale::Linear);
        assert_eq!(cfg.points[0], [4.0, 1.0]);
        assert!(cfg.transposed);
        assert!(!cfg.interactive);
        assert!(cfg.handles.is_empty());
        // Gate geometry keeps matching the swapped event coordinates.
        assert_eq!(
            cfg.gates[0].fill[0],
            [original_fill[0][1], original_fill[0][0]]
        );
    }

    #[test]
    fn graph_controls_apply_to_svg_export() {
        let mut app = app_with_synthetic_sample();
        app.update_plot_points();
        app.x_range = (0.0, 5.0);
        app.y_range = (0.0, 11.0);
        app.x_data_range = (0.0, 5.0);
        app.y_data_range = (0.0, 11.0);
        app.show_grid = true;
        app.show_medians = true;
        app.show_event_numbers = true;
        app.large_dots = true;
        app.tick_notation = crate::plot::TickNotation::Scientific;
        app.show_axis_labels = false;
        app.show_ticks = false;
        app.show_numbers = false;

        let (svg, _, _) = app.current_plot_svg().unwrap();
        assert!(
            svg.contains("#e2e8f0"),
            "background grid should be exported"
        );
        assert!(
            svg.contains("stroke-dasharray=\"5 4\""),
            "median lines should be exported"
        );
        assert!(svg.contains("n = 4"), "event count should be exported");
        assert!(svg.contains("r=\"4\""), "large dots should be exported");
        // Tick numbers are hidden, so no right-aligned Y tick labels exist.
        assert!(!svg.contains("text-anchor=\"end\""));

        // Transposed export still produces a valid plot using the swapped pair.
        app.show_axis_labels = true;
        app.show_ticks = true;
        app.show_numbers = true;
        app.transpose = true;
        let (transposed, _, _) = app.current_plot_svg().unwrap();
        assert!(transposed.starts_with("<svg"));
        assert!(transposed.ends_with("</svg>\n"));
        assert!(transposed.contains("P2 (linear)"));
        assert!(transposed.contains("P1 (linear)"));

        // Adjunct axis histograms (V6) are exported when enabled.
        app.transpose = false;
        let (plain, _, _) = app.current_plot_svg().unwrap();
        assert!(!plain.contains("axis-histograms"));
        app.show_axis_histograms = true;
        let (with_histograms, _, _) = app.current_plot_svg().unwrap();
        assert!(with_histograms.contains("id=\"axis-histograms\""));
        assert!(with_histograms.contains("fill-opacity=\"0.45\""));
        // The plot card reserves margins, so the histograms do not overlap
        // the events inside the data rectangle.
        assert!(with_histograms.len() > plain.len());
    }

    #[test]
    fn view3d_opens_and_renders_the_current_sample() {
        let mut app = app_with_synthetic_sample();
        assert!(!app.view3d_open);
        app.open_view3d();
        assert!(app.view3d_open);
        // Z is chosen distinct from the current X/Y pair.
        assert_ne!(app.view3d_spec.z, app.view3d_spec.x);
        assert_ne!(app.view3d_spec.z, app.view3d_spec.y);

        let (svg, w, h) = app.view3d_svg().unwrap();
        assert_eq!((w, h), (640, 480));
        assert!(svg.contains("<circle"));
        assert!(svg.ends_with("</svg>\n"));
    }

    #[test]
    fn view3d_preview_shapes_cover_all_modes() {
        let app = app_with_synthetic_sample();
        for mode in crate::plot3d::View3DMode::all() {
            let mut spec = app.view3d_spec.clone();
            spec.mode = mode;
            spec.x = 0;
            spec.y = 1;
            spec.z = 2;
            let fcs = app.current_fcs().unwrap();
            let shapes = crate::plot3d::preview_shapes(&fcs.data, &spec, 400, 300);
            assert!(!shapes.is_empty(), "mode {mode:?} produced no shapes");
        }
    }

    #[test]
    fn bilinear_gate_selects_region_under_line() {
        // Horizontal line at y=5 spanning x in [0, 10].
        let kind = GateKind::Bilinear {
            x1: 0.0,
            y1: 5.0,
            x2: 10.0,
            y2: 5.0,
            below: true,
        };
        let gate = kind.to_gate(0, 1);
        let data = array![[1.0, 1.0], [1.0, 9.0], [20.0, 1.0], [5.0, 5.0]];
        // (1,1) under the line & in band; (1,9) above; (20,1) outside band;
        // (5,5) exactly on the line is included.
        assert_eq!(gate.filter(&data), vec![0, 3]);
    }

    #[test]
    fn bilinear_gate_follows_a_diagonal() {
        // Line from (0,10) to (10,0): y = 10 - x.
        let kind = GateKind::Bilinear {
            x1: 0.0,
            y1: 10.0,
            x2: 10.0,
            y2: 0.0,
            below: true,
        };
        let gate = kind.to_gate(0, 1);
        let data = array![[5.0, 4.0], [5.0, 6.0], [2.0, 1.0]];
        // (5,4) below y=5 -> in; (5,6) above -> out; (2,1) below y=8 -> in.
        assert_eq!(gate.filter(&data), vec![0, 2]);
    }

    #[test]
    fn bilinear_polygon_spans_the_window_height() {
        let kind = GateKind::Bilinear {
            x1: 2.0,
            y1: 5.0,
            x2: 8.0,
            y2: 5.0,
            below: true,
        };
        let poly = kind.polygon((0.0, 10.0), (0.0, 10.0));
        // Top edge is the line; the fill drops to the bottom of the Y axis.
        assert_eq!(poly[0], [2.0, 5.0]);
        assert_eq!(poly[1], [8.0, 5.0]);
        assert_eq!(poly[2], [8.0, 0.0]);
        assert_eq!(poly[3], [2.0, 0.0]);
    }

    fn close_request_input() -> egui::RawInput {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 900.0),
            )),
            ..Default::default()
        };
        raw.viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .events
            .push(egui::ViewportEvent::Close);
        raw
    }

    fn close_canceled(output: &egui::FullOutput) -> bool {
        output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&egui::ViewportCommand::CancelClose)
    }

    #[test]
    fn closing_with_unsaved_changes_prompts_and_cancels_the_close() {
        let ctx = egui::Context::default();
        let mut app = FlowersApp {
            dirty: true,
            ..Default::default()
        };

        let output = ctx.run(close_request_input(), |ctx| app.draw(ctx));

        assert!(app.quit_confirm, "the save confirmation should be open");
        assert!(
            close_canceled(&output),
            "the window must not close before the user chooses an option"
        );
    }

    #[test]
    fn closing_a_clean_workspace_does_not_prompt() {
        let ctx = egui::Context::default();
        let mut app = FlowersApp::default();

        // The very first frame records the baseline snapshot for undo but must
        // not count as an edit.
        let mut raw = close_request_input();
        raw.viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .events
            .clear();
        let _ = ctx.run(raw, |ctx| app.draw(ctx));
        assert!(!app.dirty, "an untouched workspace must start clean");

        let output = ctx.run(close_request_input(), |ctx| app.draw(ctx));

        assert!(!app.quit_confirm);
        assert!(!close_canceled(&output), "a clean workspace should close");
    }

    #[test]
    fn quit_without_saving_allows_a_later_close_request() {
        let ctx = egui::Context::default();
        let mut app = FlowersApp {
            dirty: true,
            quit_confirm: false,
            allow_close: true,
            ..Default::default()
        };
        // This is the state the "Quit Without Saving" button sets.

        let output = ctx.run(close_request_input(), |ctx| app.draw(ctx));

        assert!(!app.quit_confirm);
        assert!(
            !close_canceled(&output),
            "the close must go through after the user opts out of saving"
        );
    }

    // Regression test for the plot / bottom gate-tools clipping bug: the
    // central panel that hosts the plot must be declared *after* the bottom
    // gate-tools panel, so egui reserves the gate-tools strip before the
    // central area is computed. Otherwise the plot area is taller than the
    // space left and the bottom of the plot is clipped by the gate tools.
    #[test]
    fn central_plot_panel_does_not_overlap_gate_editor() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));

        fn run(ctx: &egui::Context, screen: egui::Rect) -> (egui::Rect, Option<egui::Rect>) {
            let raw = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut central_rect: Option<egui::Rect> = None;
            let mut bottom_rect: Option<egui::Rect> = None;
            let _ = ctx.run(raw, |ctx| {
                egui::TopBottomPanel::bottom("gate_editor")
                    .resizable(true)
                    .default_height(200.0)
                    .show(ctx, |ui| {
                        bottom_rect = Some(ui.min_rect());
                        ui.label("gate tools");
                    });
                egui::CentralPanel::default().show(ctx, |ui| {
                    central_rect = Some(ui.min_rect());
                    ui.label("plot area");
                });
            });
            (central_rect.unwrap(), bottom_rect)
        }

        let (central, bottom) = run(&ctx, screen);
        let bottom = bottom.expect("bottom gate editor panel must be present");
        assert!(
            central.max.y <= bottom.min.y,
            "central plot panel must end above the bottom gate tools panel \
             (central={central:?}, bottom={bottom:?})"
        );
    }
}
