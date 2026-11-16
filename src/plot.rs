//! Rendering helpers for flow cytometry plots.
//!
//! This module has two halves:
//!
//! * **SVG export** — [`dot_plot_svg`] and [`histogram_svg`] produce standalone
//!   `.svg` files without any external graphics dependency, so `flowers` can
//!   run headless (CLI and "layout editor" exports).
//! * **Analysis helpers** — [`density_grid`], [`point_densities`], and
//!   [`contour_segments`] compute 2D kernel/histogram densities, per-point
//!   densities (for gradient point colouring) and topographic contour lines.
//!   These are shared by the GUI and the SVG renderer.

use serde::Serialize;

pub use crate::transform::AxisScale;

/// How tick numbers are formatted (V5 graph controls).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum TickNotation {
    /// Compact decimals for small values, scientific notation for very large
    /// or very small values.
    #[default]
    Auto,
    /// Always use scientific notation (e.g. `1.5e4`).
    Scientific,
    /// Always use plain decimals (no exponent).
    Decimal,
}

impl TickNotation {
    /// All notations, in display order.
    pub fn all() -> [Self; 3] {
        [
            TickNotation::Auto,
            TickNotation::Scientific,
            TickNotation::Decimal,
        ]
    }

    /// Stable identifier used by project files.
    pub fn id(self) -> &'static str {
        match self {
            TickNotation::Auto => "auto",
            TickNotation::Scientific => "scientific",
            TickNotation::Decimal => "decimal",
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            TickNotation::Auto => "Auto",
            TickNotation::Scientific => "Scientific",
            TickNotation::Decimal => "Decimal",
        }
    }

    /// Parses [`Self::id`].
    pub fn from_id(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(TickNotation::Auto),
            "scientific" | "sci" => Some(TickNotation::Scientific),
            "decimal" | "plain" => Some(TickNotation::Decimal),
            _ => None,
        }
    }
}

/// Styling for a rendered plot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlotStyle {
    pub width: u32,
    pub height: u32,
    pub point_radius: f32,
    pub point_opacity: f32,
    pub point_color: String,
    pub line_color: String,
    pub axis_color: String,
    pub text_color: String,
    pub background: String,
    /// Scale of the X axis.
    pub x_scale: AxisScale,
    /// Scale of the Y axis.
    pub y_scale: AxisScale,
    /// Colour events by local density (gradient layering).
    pub gradient: bool,
    /// Render a topographic contour view instead of individual points.
    pub contour: bool,
    /// Number of contour levels used when `contour` is enabled.
    pub contour_levels: usize,
    /// Optional plot title.
    pub title: String,
    /// Font size (in SVG units) of the X-axis label.
    pub x_label_size: f32,
    /// Font size (in SVG units) of the Y-axis label.
    pub y_label_size: f32,
    /// Rotation of the X-axis label, in degrees (clockwise).
    pub x_label_rotation: f32,
    /// Rotation of the Y-axis label, in degrees (clockwise).
    pub y_label_rotation: f32,
    /// CSS font family used for the plot title.
    pub label_font: String,
    /// CSS font family used for the X-axis label.
    pub x_label_font: String,
    /// CSS font family used for the Y-axis label.
    pub y_label_font: String,
    /// Font size (in SVG units) of the plot title.
    pub title_size: f32,
    /// When false, the `(linear)` / `(log10)` suffix is omitted from axis labels.
    pub show_scale_labels: bool,
    // -- V5 graph controls -------------------------------------------------
    /// Density smoothing in `[0, 1]`; each step applies one 3x3 blur pass.
    pub smoothing: f32,
    /// Density grid resolution (cells per axis) for gradient/contour views.
    pub resolution: usize,
    /// Saturate the density colour scale at this top percentage of events
    /// (`0` disables outlier clipping).
    pub outlier_clip_percent: f64,
    /// Draw events at twice the normal radius.
    pub large_dots: bool,
    /// Draw a background grid at the major tick positions.
    pub grid: bool,
    /// Draw median lines across the plot.
    pub medians: bool,
    /// Show the number of plotted events in the top-left corner.
    pub show_event_numbers: bool,
    /// Append population frequencies to data-overlay legend entries.
    pub show_names_frequencies: bool,
    /// Tick-number notation.
    pub notation: TickNotation,
    /// Show the X/Y axis labels.
    pub show_axis_labels: bool,
    /// Draw tick marks on the axes.
    pub show_ticks: bool,
    /// Draw tick numbers next to the axes.
    pub show_numbers: bool,
    /// Draw adjunct histograms along the top (X) and right (Y) axes (V6).
    pub axis_histograms: bool,
}

impl Default for PlotStyle {
    fn default() -> Self {
        Self {
            width: 500,
            height: 450,
            point_radius: 2.0,
            point_opacity: 0.6,
            point_color: "#1f77b4".to_string(),
            line_color: "#2ca02c".to_string(),
            axis_color: "#444444".to_string(),
            text_color: "#222222".to_string(),
            background: "#ffffff".to_string(),
            x_scale: AxisScale::Linear,
            y_scale: AxisScale::Linear,
            gradient: false,
            contour: false,
            contour_levels: 8,
            title: String::new(),
            x_label_size: 12.0,
            y_label_size: 12.0,
            x_label_rotation: 0.0,
            y_label_rotation: -90.0,
            label_font: "sans-serif".to_string(),
            x_label_font: "sans-serif".to_string(),
            y_label_font: "sans-serif".to_string(),
            title_size: 14.0,
            show_scale_labels: true,
            smoothing: 0.0,
            resolution: 128,
            outlier_clip_percent: 0.0,
            large_dots: false,
            grid: false,
            medians: false,
            show_event_numbers: false,
            show_names_frequencies: false,
            notation: TickNotation::Auto,
            show_axis_labels: true,
            show_ticks: true,
            show_numbers: true,
            axis_histograms: false,
        }
    }
}

/// A pre-computed overlay (gate outline) to draw on top of an SVG plot.
#[derive(Debug, Clone)]
pub struct PlotOverlay {
    /// Vertices in data coordinates.
    pub points: Vec<[f64; 2]>,
    /// Stroke colour (CSS colour string).
    pub stroke: String,
    /// Optional fill colour (CSS colour string).
    pub fill: Option<String>,
    /// Whether the path should be closed.
    pub closed: bool,
    /// Optional population label drawn at a data-space position.
    pub label: Option<OverlayLabel>,
    /// Draw the outline dashed (used for backgated ancestor gates).
    pub dashed: bool,
}

/// A text label attached to a gate overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayLabel {
    /// Label text (single line).
    pub text: String,
    /// Anchor position in data coordinates.
    pub position: [f64; 2],
    /// Text colour as a CSS colour string.
    pub color: String,
    /// Font size in SVG units (and egui points in the interactive plot).
    pub size: f32,
}

/// How a data overlay (an extra sample or population) is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayKind {
    /// Individual events as dots.
    #[default]
    Dots,
    /// Raw (uncompensated) events as dots, shown next to compensated data (C4).
    Raw,
    /// Topographic contour lines.
    Contour,
    /// A step histogram along the shared X axis.
    Histogram,
    /// Density-coloured events.
    Pseudocolor,
}

/// A data layer drawn on shared axes: an extra sample or population.
///
/// Data overlays are rendered underneath gate outlines so that gating stays
/// legible when several populations are overlaid in one graph (V2).
#[derive(Debug, Clone)]
pub struct DataOverlay {
    /// Legend label.
    pub label: String,
    /// CSS colour used for dots, contours and histogram lines.
    pub color: String,
    /// Events in data coordinates.
    pub points: Vec<[f64; 2]>,
    /// How to render this layer.
    pub kind: OverlayKind,
    /// Number of bins for histogram overlays.
    pub bins: usize,
    /// Colour map for pseudocolour overlays.
    pub density_map: DensityMap,
}

impl DataOverlay {
    /// Creates a dot overlay with the default pseudocolour map.
    pub fn dots(label: impl Into<String>, color: impl Into<String>, points: Vec<[f64; 2]>) -> Self {
        Self {
            label: label.into(),
            color: color.into(),
            points,
            kind: OverlayKind::Dots,
            bins: 32,
            density_map: DensityMap::Rainbow,
        }
    }

    /// Creates a step-histogram overlay (V2 overlay creation).
    pub fn histogram(
        label: impl Into<String>,
        color: impl Into<String>,
        points: Vec<[f64; 2]>,
        bins: usize,
    ) -> Self {
        Self {
            label: label.into(),
            color: color.into(),
            points,
            kind: OverlayKind::Histogram,
            bins: bins.max(2),
            density_map: DensityMap::Rainbow,
        }
    }

    /// Creates a contour overlay (V2 overlay creation).
    pub fn contour(
        label: impl Into<String>,
        color: impl Into<String>,
        points: Vec<[f64; 2]>,
    ) -> Self {
        Self {
            label: label.into(),
            color: color.into(),
            points,
            kind: OverlayKind::Contour,
            bins: 32,
            density_map: DensityMap::Rainbow,
        }
    }

    /// Creates a pseudocolour overlay (V2 overlay creation).
    pub fn pseudocolor(label: impl Into<String>, points: Vec<[f64; 2]>, map: DensityMap) -> Self {
        Self {
            label: label.into(),
            color: "#1f77b4".into(),
            points,
            kind: OverlayKind::Pseudocolor,
            bins: 32,
            density_map: map,
        }
    }

    /// Creates a raw-event overlay (V2 overlay creation; C4 raw view).
    pub fn raw(label: impl Into<String>, color: impl Into<String>, points: Vec<[f64; 2]>) -> Self {
        Self {
            label: label.into(),
            color: color.into(),
            points,
            kind: OverlayKind::Raw,
            bins: 32,
            density_map: DensityMap::Rainbow,
        }
    }

    /// Replace the rendering kind while keeping label/colour/points.
    pub fn with_kind(mut self, kind: OverlayKind) -> Self {
        self.kind = kind;
        self
    }
}

/// Extract `(x, y)` points for an overlay from an event matrix (V2 overlay
/// creation). An empty `indices` selects all events.
pub fn overlay_points(
    data: &ndarray::Array2<f32>,
    x_idx: usize,
    y_idx: usize,
    indices: &[usize],
) -> Vec<[f64; 2]> {
    let rows = data.nrows();
    let cols = data.ncols();
    if x_idx >= cols || y_idx >= cols {
        return Vec::new();
    }
    let selected: Vec<usize> = if indices.is_empty() {
        (0..rows).collect()
    } else {
        indices
            .iter()
            .copied()
            .filter(|index| *index < rows)
            .collect()
    };
    selected
        .into_iter()
        .map(|row| [data[[row, x_idx]] as f64, data[[row, y_idx]] as f64])
        .collect()
}

/// Build a data overlay from an event matrix in one call (V2 overlay creation).
#[allow(clippy::too_many_arguments)]
pub fn data_overlay(
    label: impl Into<String>,
    data: &ndarray::Array2<f32>,
    x_idx: usize,
    y_idx: usize,
    indices: &[usize],
    kind: OverlayKind,
    color: impl Into<String>,
) -> DataOverlay {
    let points = overlay_points(data, x_idx, y_idx, indices);
    let mut overlay = DataOverlay::dots(label, color, points);
    overlay.kind = kind;
    overlay
}

/// Which pieces of information a gate label shows.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GateLabelSettings {
    /// Master switch for population labels.
    pub show: bool,
    /// Include the population name.
    pub name: bool,
    /// Include the event count.
    pub count: bool,
    /// Include the percentage of the parent population.
    pub percent_parent: bool,
    /// Include the percentage of all events.
    pub percent_total: bool,
    /// Include the full ancestry path.
    pub ancestry: bool,
    /// Draw outlines of the selected gate's ancestors (backgating).
    pub backgating: bool,
    /// Draw the parent events that fall outside the selected gate, i.e. what
    /// would be included if the gate were omitted (V4).
    #[serde(default)]
    pub backgate_events: bool,
}

impl Default for GateLabelSettings {
    fn default() -> Self {
        Self {
            show: true,
            name: true,
            count: true,
            percent_parent: true,
            percent_total: true,
            ancestry: false,
            backgating: false,
            backgate_events: false,
        }
    }
}

/// Everything needed to format a population label for a gate.
#[derive(Debug, Clone, PartialEq)]
pub struct PopulationLabel {
    pub name: String,
    /// Events in this population.
    pub count: usize,
    /// Events in the parent population, when the gate has a parent.
    pub parent_count: Option<usize>,
    /// Events in the sample (denominator for % of total).
    pub total_count: usize,
    /// Ancestor names from the root to this population (inclusive).
    pub ancestry: Vec<String>,
}

impl PopulationLabel {
    /// Format the label according to `settings`.
    ///
    /// Percentages use one decimal place and are omitted when the relevant
    /// denominator is zero. The empty string means "draw no label".
    pub fn text(&self, settings: &GateLabelSettings) -> String {
        if !settings.show {
            return String::new();
        }
        let mut parts = Vec::new();
        if settings.name && !self.name.is_empty() {
            parts.push(self.name.clone());
        }
        if settings.count {
            parts.push(format!("n={}", self.count));
        }
        let percent = |numerator: usize, denominator: usize| -> Option<String> {
            (denominator > 0)
                .then(|| format!("{:.1}%", numerator as f64 / denominator as f64 * 100.0))
        };
        if settings.percent_parent {
            let denominator = self.parent_count.unwrap_or(self.total_count);
            if let Some(pct) = percent(self.count, denominator) {
                parts.push(format!("{pct} parent"));
            }
        }
        if settings.percent_total
            && let Some(pct) = percent(self.count, self.total_count)
        {
            parts.push(format!("{pct} total"));
        }
        if settings.ancestry && !self.ancestry.is_empty() {
            parts.push(self.ancestry.join(" / "));
        }
        parts.join("  ·  ")
    }
}

/// Ancestor gate indices of `selected`, ordered root first and excluding
/// `selected` itself. Cycles and out-of-range parents stop the walk safely.
pub fn backgate_ancestors(parents: &[Option<usize>], selected: usize) -> Vec<usize> {
    let mut chain = Vec::new();
    let mut seen = std::collections::HashSet::new();
    seen.insert(selected);
    let mut current = parents.get(selected).copied().flatten();
    while let Some(index) = current {
        if index >= parents.len() || !seen.insert(index) {
            break;
        }
        chain.push(index);
        current = parents.get(index).copied().flatten();
    }
    chain.reverse();
    chain
}

/// Escapes a string for safe inclusion in an SVG text node.
fn svg_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Builds an axis label, optionally appending the `(linear)` / `(log10)` suffix.
fn label_with_scale(label: &str, scale: &AxisScale, show_scale: bool) -> String {
    if show_scale {
        format!("{} ({})", label, scale.label())
    } else {
        label.to_string()
    }
}

/// An SVG `transform="rotate(...)"` attribute about `(x, y)`, or an empty string
/// when the rotation is effectively zero.
fn rotation_attr(deg: f32, (x, y): (f64, f64)) -> String {
    if deg.abs() < 1e-6 {
        String::new()
    } else {
        format!(" transform=\"rotate({deg} {x},{y})\"")
    }
}

/// Formats an axis tick value for display given an axis scale.
///
/// Large values are rendered in scientific notation (e.g. `7e4` for 70000) so
/// that tick labels do not collide with the axis title.
pub fn format_tick(v: f64, scale: &AxisScale) -> String {
    format_tick_with(v, scale, TickNotation::Auto)
}

/// Formats a plot-space tick with an explicit notation (V5).
pub fn format_tick_with(v: f64, scale: &AxisScale, notation: TickNotation) -> String {
    format_value_with(scale.inverse(v), notation)
}

/// Formats a data-space value with an explicit notation.
pub fn format_value_with(value: f64, notation: TickNotation) -> String {
    if !value.is_finite() {
        return "".to_string();
    }
    if notation == TickNotation::Scientific {
        return scientific_tick(value);
    }
    if notation == TickNotation::Decimal {
        return decimal_tick(value);
    }
    let abs = value.abs();
    if abs >= 1000.0 || (abs > 0.0 && abs < 0.01) {
        scientific_tick(value)
    } else if abs >= 100.0 {
        format!("{value:.0}")
    } else if abs >= 10.0 {
        format!("{value:.1}")
    } else if abs >= 1.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
    }
}

fn scientific_tick(value: f64) -> String {
    let abs = value.abs();
    if abs == 0.0 {
        return "0".to_string();
    }
    {
        // Scientific notation: one significant digit, strip trailing ".0".
        let mut s = format!("{value:.1e}");
        // "7.0e4" -> "7e4", "7.5e4" stays "7.5e4". Replace the whole ".0e"
        // run so the exponent marker is not duplicated.
        if let Some(pos) = s.find(".0e") {
            s.replace_range(pos..pos + 3, "e");
        }
        // Normalise exponent: "7e+04" -> "7e4".
        if let Some(e_pos) = s.find('e') {
            let rest = &s[e_pos + 1..];
            let normalized = rest.trim_start_matches('+').trim_start_matches('0');
            let normalized = if normalized.is_empty() {
                "0"
            } else {
                normalized
            };
            s = format!("{}e{}", &s[..e_pos], normalized);
        }
        s
    }
}

fn decimal_tick(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let abs = value.abs();
    let decimals = if abs >= 1000.0 {
        1
    } else if abs >= 100.0 {
        0
    } else if abs >= 10.0 {
        1
    } else if abs >= 1.0 {
        2
    } else if abs >= 0.01 {
        4
    } else {
        6
    };
    let mut s = format!("{value:.decimals$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

/// Renders a 2D dot plot of two parameters for a set of events.
///
/// `data` has shape `(events, parameters)`. `x_idx` / `y_idx` select the
/// columns. `indices` selects which events to draw (if empty, all events).
/// `x_range` / `y_range` are the axis limits in **data** coordinates; if `None`
/// they are computed from the selected data. `overlays` are gate outlines drawn
/// on top of the events.
#[allow(clippy::too_many_arguments)]
/// Density colour maps for the pseudocolour and zebra views (V1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DensityMap {
    #[default]
    Rainbow,
    Jet,
    Viridis,
    Greyscale,
    Zebra,
}

impl DensityMap {
    /// All density maps, in display order.
    pub fn all() -> Vec<Self> {
        vec![
            DensityMap::Rainbow,
            DensityMap::Jet,
            DensityMap::Viridis,
            DensityMap::Greyscale,
            DensityMap::Zebra,
        ]
    }

    /// Look up a density map by id (`"rainbow"`, `"jet"`, `"viridis"`, `"greyscale"`, `"zebra"`).
    pub fn from_id(id: &str) -> Option<Self> {
        match id.trim().to_ascii_lowercase().as_str() {
            "rainbow" => Some(DensityMap::Rainbow),
            "jet" => Some(DensityMap::Jet),
            "viridis" => Some(DensityMap::Viridis),
            "greyscale" | "gray" | "grey" => Some(DensityMap::Greyscale),
            "zebra" => Some(DensityMap::Zebra),
            _ => None,
        }
    }

    /// Stable identifier of this map.
    pub fn id(&self) -> &'static str {
        match self {
            DensityMap::Rainbow => "rainbow",
            DensityMap::Jet => "jet",
            DensityMap::Viridis => "viridis",
            DensityMap::Greyscale => "greyscale",
            DensityMap::Zebra => "zebra",
        }
    }

    /// Human-readable name of this map.
    pub fn label(&self) -> &'static str {
        self.id()
    }

    /// Sample the map at normalized density `t` in `[0, 1]`, returning RGB in
    /// `[0, 1]`.
    fn sample(&self, t: f64) -> (f64, f64, f64) {
        match self {
            DensityMap::Greyscale => (t, t, t),
            DensityMap::Zebra => {
                let k = (t * 2.0).floor() as i32;
                if k % 2 == 0 {
                    (0.85, 0.85, 0.85)
                } else {
                    (0.15, 0.15, 0.15)
                }
            }
            DensityMap::Rainbow | DensityMap::Jet | DensityMap::Viridis => {
                let stops: &[(f64, (f64, f64, f64))] = match self {
                    DensityMap::Rainbow => &[
                        (0.0, (0.0, 0.0, 0.55)),
                        (0.35, (0.0, 0.745, 0.863)),
                        (0.7, (0.98, 0.863, 0.157)),
                        (1.0, (0.863, 0.078, 0.078)),
                    ],
                    DensityMap::Jet => &[
                        (0.0, (0.0, 0.0, 0.0)),
                        (0.5, (0.0, 0.0, 1.0)),
                        (1.0, (1.0, 0.0, 0.0)),
                    ],
                    DensityMap::Viridis => &[
                        (0.0, (0.267, 0.129, 0.533)),
                        (0.25, (0.227, 0.506, 0.741)),
                        (0.5, (0.129, 0.792, 0.835)),
                        (0.75, (0.584, 0.816, 0.388)),
                        (1.0, (0.992, 0.882, 0.345)),
                    ],
                    DensityMap::Greyscale => &[(0.0, (0.0, 0.0, 0.0))],
                    DensityMap::Zebra => &[(0.0, (0.0, 0.0, 0.0))],
                };
                for w in stops.windows(2) {
                    let (t0, c0) = w[0];
                    let (t1, c1) = w[1];
                    if t >= t0 && t <= t1 {
                        let f = if (t1 - t0).abs() < f64::EPSILON {
                            0.0
                        } else {
                            (t - t0) / (t1 - t0)
                        };
                        return (
                            c0.0 + (c1.0 - c0.0) * f,
                            c0.1 + (c1.1 - c0.1) * f,
                            c0.2 + (c1.2 - c0.2) * f,
                        );
                    }
                }
                let (_t0, c0) = stops[stops.len() - 1];
                (c0.0, c0.1, c0.2)
            }
        }
    }

    /// Colour at normalized density `t` in `[0, 1]`.
    pub fn sample_colour(&self, t: f64) -> (f64, f64, f64) {
        self.sample(t)
    }

    /// Map colour as RGB in `[0, 1]`.
    fn colour(&self, t: f64) -> (f64, f64, f64) {
        self.sample(t)
    }

    /// Map colour as `u8` RGB.
    fn colour_u8(&self, t: f64) -> (u8, u8, u8) {
        let (r, g, b) = self.sample(t);
        (
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8,
        )
    }
}

/// Density colour of a map at normalized density `t` (RGB in `[0, 1]`).
pub fn density_colour(map: &DensityMap, t: f64) -> (f64, f64, f64) {
    map.sample_colour(t)
}

/// Hex `#rrggbb` of a map colour at normalized density `t`.
pub fn density_hex_for(map: &DensityMap, t: f64) -> String {
    let (r, g, b) = map.colour_u8(t);
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

/// SVG legend for a density map: a horizontal gradient bar.
pub fn density_legend_svg(map: &DensityMap, width: f32) -> String {
    let (r0, g0, b0) = map.colour(0.0);
    let (r1, g1, b1) = map.colour(1.0);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"12\"><defs><linearGradient id=\"lg\" x1=\"0\" x2=\"1\"><stop offset=\"0\" stop-color=\"rgb({r0}, {g0}, {b0})\"/><stop offset=\"1\" stop-color=\"rgb({r1}, {g1}, {b1})\"/></linearGradient></defs><rect width=\"{width}\" height=\"12\" fill=\"url(#lg)\"/></svg>"
    )
}

#[allow(clippy::too_many_arguments)]
fn dot_plot_svg_impl(
    data: &ndarray::Array2<f32>,
    x_idx: usize,
    y_idx: usize,
    x_label: &str,
    y_label: &str,
    indices: &[usize],
    x_range: Option<(f32, f32)>,
    y_range: Option<(f32, f32)>,
    overlays: &[PlotOverlay],
    data_overlays: &[DataOverlay],
    style: &PlotStyle,
    density: Option<(&DensityMap, usize)>,
) -> String {
    let (_, n_params) = data.dim();
    assert!(x_idx < n_params && y_idx < n_params);

    let sel: Vec<usize> = if indices.is_empty() {
        (0..data.nrows()).collect()
    } else {
        indices.to_vec()
    };

    let raw_x: Vec<f64> = sel.iter().map(|&i| data[[i, x_idx]] as f64).collect();
    let raw_y: Vec<f64> = sel.iter().map(|&i| data[[i, y_idx]] as f64).collect();

    // Resolve the data-space ranges. When auto-fitting, include every data
    // overlay so all layers share one axis range.
    let mut fit_x = raw_x.clone();
    let mut fit_y = raw_y.clone();
    for overlay in data_overlays {
        for p in &overlay.points {
            fit_x.push(p[0]);
            fit_y.push(p[1]);
        }
    }
    let (dx0, dx1) = x_range.unwrap_or_else(|| {
        let (a, b) = compute_range(&fit_x, &style.x_scale);
        (a as f32, b as f32)
    });
    let (dy0, dy1) = y_range.unwrap_or_else(|| {
        let (a, b) = compute_range(&fit_y, &style.y_scale);
        (a as f32, b as f32)
    });
    let (dx0, dx1) = (dx0 as f64, dx1 as f64);
    let (dy0, dy1) = (dy0 as f64, dy1 as f64);

    // Transform ranges into plot space.
    let x0t = style.x_scale.transform(dx0).unwrap_or(0.0);
    let x1t = style.x_scale.transform(dx1).unwrap_or(1.0);
    let y0t = style.y_scale.transform(dy0).unwrap_or(0.0);
    let y1t = style.y_scale.transform(dy1).unwrap_or(1.0);
    let (min_x, max_x) = pad_span(x0t, x1t);
    let (min_y, max_y) = pad_span(y0t, y1t);

    // Transform points and keep only those inside the range.
    let mut pts: Vec<[f64; 2]> = Vec::with_capacity(sel.len());
    for i in 0..raw_x.len() {
        let (Some(px), Some(py)) = (
            style.x_scale.transform(raw_x[i]),
            style.y_scale.transform(raw_y[i]),
        ) else {
            continue;
        };
        if px < min_x || px > max_x || py < min_y || py > max_y {
            continue;
        }
        pts.push([px, py]);
    }

    let use_density = density.is_some() || style.gradient || style.contour;
    let resolution = style.resolution.clamp(16, 512);
    let mut grid = if use_density {
        Some(density_grid(
            &pts, min_x, max_x, min_y, max_y, resolution, resolution,
        ))
    } else {
        None
    };
    if let Some(g) = grid.as_mut() {
        smooth_density_grid(g, style.smoothing);
    }
    // Outlier clipping changes the colour/contour reference, not the data.
    let densities = grid.as_ref().map(|g| {
        let dens = point_densities(&pts, g);
        let max_d = clipped_density_max(&dens, style.outlier_clip_percent);
        (max_d, dens)
    });

    // Reserve a top band for the X histogram and a right band for the Y
    // histogram when adjunct axis histograms are enabled (V6).
    let histograms = style.axis_histograms;
    let plot_w = style.width as f64 - if histograms { 120.0 } else { 80.0 };
    let plot_h = style.height as f64 - if histograms { 110.0 } else { 80.0 };
    let origin_x = 60.0;
    let origin_y = style.height as f64 - 40.0;

    let sx = move |v: f64| origin_x + (v - min_x) / (max_x - min_x) * plot_w;
    let sy = move |v: f64| origin_y - (v - min_y) / (max_y - min_y) * plot_h;

    let mut svg = String::new();
    svg.push_str(&svg_header(style.width, style.height, &style.background));
    svg.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{}\" />\n",
        style.width, style.height, style.background
    ));

    // Axes.
    svg.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1\" />\n",
        origin_x,
        origin_y,
        origin_x + plot_w,
        origin_y,
        style.axis_color
    ));
    svg.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1\" />\n",
        origin_x,
        origin_y,
        origin_x,
        origin_y - plot_h,
        style.axis_color
    ));

    // Title.
    if !style.title.is_empty() {
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"22\" font-family=\"{}\" font-size=\"{}\" font-weight=\"bold\" fill=\"{}\" text-anchor=\"middle\">{}</text>\n",
            style.width as f64 / 2.0,
            svg_escape(&style.label_font),
            style.title_size,
            style.text_color,
            svg_escape(&style.title)
        ));
    }

    let y_center = origin_y - plot_h / 2.0;
    // Axis labels.
    let x_anchor = (origin_x + plot_w / 2.0, style.height as f64 - 10.0);
    // The Y label rotates about its anchor (the label centre), so anchor it just
    // left of the y-axis with the same margin the X label has below the x-axis,
    // clear of the y-axis tick numbers.
    let y_anchor = (origin_x - 30.0, y_center);
    let x_text = label_with_scale(x_label, &style.x_scale, style.show_scale_labels);
    let y_text = label_with_scale(y_label, &style.y_scale, style.show_scale_labels);
    if style.show_axis_labels {
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\" text-anchor=\"middle\"{}>{}</text>\n",
            x_anchor.0,
            x_anchor.1,
            svg_escape(&style.x_label_font),
            style.x_label_size,
            style.text_color,
            rotation_attr(style.x_label_rotation, x_anchor),
            svg_escape(&x_text)
        ));
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\" text-anchor=\"middle\"{}>{}</text>\n",
            y_anchor.0,
            y_anchor.1,
            svg_escape(&style.y_label_font),
            style.y_label_size,
            style.text_color,
            rotation_attr(style.y_label_rotation, y_anchor),
            svg_escape(&y_text)
        ));
    }

    // Tick labels.
    let xticks = 5;
    let yticks = 5;
    for t in 0..=xticks {
        let v = min_x + (max_x - min_x) * (t as f64 / xticks as f64);
        let px = sx(v);
        if style.show_numbers {
            svg.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-size=\"10\" fill=\"{}\" text-anchor=\"middle\">{}</text>\n",
                px,
                origin_y + 15.0,
                style.text_color,
                svg_escape(&format_tick_with(v, &style.x_scale, style.notation))
            ));
        }
        if style.show_ticks {
            svg.push_str(&format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"0.5\" />\n",
                px,
                origin_y,
                px,
                origin_y - 5.0,
                style.axis_color
            ));
        }
    }
    for t in 0..=yticks {
        let v = min_y + (max_y - min_y) * (t as f64 / yticks as f64);
        let py = sy(v);
        if style.show_numbers {
            svg.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-size=\"10\" fill=\"{}\" text-anchor=\"end\">{}</text>\n",
                origin_x - 8.0,
                py + 4.0,
                style.text_color,
                svg_escape(&format_tick_with(v, &style.y_scale, style.notation))
            ));
        }
        if style.show_ticks {
            svg.push_str(&format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"0.5\" />\n",
                origin_x - 5.0,
                py,
                origin_x,
                py,
                style.axis_color
            ));
        }
    }

    // Background grid (V5): light lines at the major tick positions.
    if style.grid {
        for t in 0..=xticks {
            let px = sx(min_x + (max_x - min_x) * (t as f64 / xticks as f64));
            svg.push_str(&format!(
                "<line x1=\"{px:.2}\" y1=\"{:.2}\" x2=\"{px:.2}\" y2=\"{:.2}\" stroke=\"#e2e8f0\" stroke-width=\"0.6\" />\n",
                origin_y - plot_h,
                origin_y
            ));
        }
        for t in 0..=yticks {
            let py = sy(min_y + (max_y - min_y) * (t as f64 / yticks as f64));
            svg.push_str(&format!(
                "<line x1=\"{:.2}\" y1=\"{py:.2}\" x2=\"{:.2}\" y2=\"{py:.2}\" stroke=\"#e2e8f0\" stroke-width=\"0.6\" />\n",
                origin_x,
                origin_x + plot_w
            ));
        }
    }

    // Median lines (V5).
    if style.medians {
        if let Some(mx) = median_of(&raw_x)
            .and_then(|v| style.x_scale.transform(v))
            .filter(|v| *v >= min_x && *v <= max_x)
        {
            svg.push_str(&format!(
                "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"{}\" stroke-width=\"1\" stroke-dasharray=\"5 4\" stroke-opacity=\"0.8\" />\n",
                sx(mx),
                origin_y - plot_h,
                sx(mx),
                origin_y,
                style.point_color
            ));
        }
        if let Some(my) = median_of(&raw_y)
            .and_then(|v| style.y_scale.transform(v))
            .filter(|v| *v >= min_y && *v <= max_y)
        {
            svg.push_str(&format!(
                "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"{}\" stroke-width=\"1\" stroke-dasharray=\"5 4\" stroke-opacity=\"0.8\" />\n",
                origin_x,
                sy(my),
                origin_x + plot_w,
                sy(my),
                style.point_color
            ));
        }
    }

    // Event count annotation (V5).
    if style.show_event_numbers {
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" font-family=\"{}\" fill=\"{}\" text-anchor=\"start\">n = {}</text>\n",
            origin_x + 6.0,
            origin_y - plot_h + 16.0,
            svg_escape(&style.label_font),
            style.text_color,
            pts.len()
        ));
    }

    // Adjunct axis histograms (V6): X along the top, Y along the right.
    if histograms {
        let xs: Vec<f64> = pts.iter().map(|p| p[0]).collect();
        let ys: Vec<f64> = pts.iter().map(|p| p[1]).collect();
        let bins = 48;
        let xcounts = axis_histogram_counts(&xs, min_x, max_x, bins);
        let ycounts = axis_histogram_counts(&ys, min_y, max_y, bins);
        let xmax = xcounts.iter().copied().max().unwrap_or(1).max(1) as f64;
        let ymax = ycounts.iter().copied().max().unwrap_or(1).max(1) as f64;
        let bin_w = plot_w / bins as f64;
        let bin_h = plot_h / bins as f64;
        svg.push_str("<g id=\"axis-histograms\">\n");
        let base_y = origin_y - plot_h - 4.0;
        for (b, &count) in xcounts.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let h = count as f64 / xmax * 24.0;
            svg.push_str(&format!(
                "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"{}\" fill-opacity=\"0.45\" />\n",
                origin_x + b as f64 * bin_w,
                base_y - h,
                bin_w.max(1.0) - 0.5,
                h,
                style.point_color
            ));
        }
        let base_x = origin_x + plot_w + 4.0;
        for (b, &count) in ycounts.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let w = count as f64 / ymax * 40.0;
            svg.push_str(&format!(
                "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"{}\" fill-opacity=\"0.45\" />\n",
                base_x,
                origin_y - (b + 1) as f64 * bin_h,
                w,
                bin_h.max(1.0) - 0.5,
                style.point_color
            ));
        }
        svg.push_str("</g>\n");
    }

    // Events. `large_dots` doubles the marker radius for all event layers.
    let point_radius = style.point_radius * if style.large_dots { 2.0 } else { 1.0 };
    if style.contour {
        if let Some(grid) = grid.as_ref() {
            let max_density = clipped_density_max(&grid.values, style.outlier_clip_percent);
            for level in contour_levels(max_density, style.contour_levels) {
                for seg in contour_segments(grid, level) {
                    let a = seg[0];
                    let b = seg[1];
                    svg.push_str(&format!(
                        "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"{}\" stroke-width=\"1.2\" stroke-opacity=\"0.85\" />\n",
                        sx(a[0]),
                        sy(a[1]),
                        sx(b[0]),
                        sy(b[1]),
                        style.line_color
                    ));
                }
            }
        }
    } else {
        for (i, p) in pts.iter().enumerate() {
            let px = sx(p[0]);
            let py = sy(p[1]);
            let fill = if let Some((map, _bands)) = density {
                let t = densities
                    .as_ref()
                    .and_then(|d| {
                        if d.0 > 0.0 {
                            Some((d.1[i] / d.0).clamp(0.0, 1.0))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0.0);
                density_hex_for(map, t)
            } else if style.gradient {
                if let Some((max_density, dens)) = &densities {
                    if *max_density > 0.0 {
                        let t = (dens[i] / max_density).clamp(0.0, 1.0);
                        density_hex(t)
                    } else {
                        style.point_color.clone()
                    }
                } else {
                    style.point_color.clone()
                }
            } else {
                style.point_color.clone()
            };
            svg.push_str(&format!(
                "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{}\" fill=\"{}\" fill-opacity=\"{}\" />\n",
                px, py, point_radius, fill, style.point_opacity
            ));
        }
    }

    // Data overlays (extra samples / populations) drawn under gate outlines.
    for overlay in data_overlays {
        // Transform and range-filter this layer's events.
        let mut opts: Vec<[f64; 2]> = Vec::with_capacity(overlay.points.len());
        for p in &overlay.points {
            let (Some(px), Some(py)) =
                (style.x_scale.transform(p[0]), style.y_scale.transform(p[1]))
            else {
                continue;
            };
            if px < min_x || px > max_x || py < min_y || py > max_y {
                continue;
            }
            opts.push([px, py]);
        }
        if opts.is_empty() {
            continue;
        }
        match overlay.kind {
            OverlayKind::Dots => {
                for p in &opts {
                    svg.push_str(&format!(
                        "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{}\" fill=\"{}\" fill-opacity=\"0.7\" />\n",
                        sx(p[0]),
                        sy(p[1]),
                        point_radius,
                        overlay.color
                    ));
                }
            }
            OverlayKind::Raw => {
                // Raw events are drawn as plain dots, in the overlay colour.
                for p in &opts {
                    svg.push_str(&format!(
                        "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{}\" fill=\"{}\" fill-opacity=\"0.7\" />\n",
                        sx(p[0]),
                        sy(p[1]),
                        point_radius,
                        overlay.color
                    ));
                }
            }
            OverlayKind::Pseudocolor => {
                let g = density_grid(&opts, min_x, max_x, min_y, max_y, 128, 128);
                let max_d = g.max();
                let dens = point_densities(&opts, &g);
                for (i, p) in opts.iter().enumerate() {
                    let t = if max_d > 0.0 {
                        (dens[i] / max_d).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    svg.push_str(&format!(
                        "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{}\" fill=\"{}\" fill-opacity=\"0.85\" />\n",
                        sx(p[0]),
                        sy(p[1]),
                        point_radius,
                        density_hex_for(&overlay.density_map, t)
                    ));
                }
            }
            OverlayKind::Contour => {
                let g = density_grid(&opts, min_x, max_x, min_y, max_y, 96, 96);
                let max_d = g.max();
                for level in contour_levels(max_d, style.contour_levels.max(3)) {
                    for seg in contour_segments(&g, level) {
                        svg.push_str(&format!(
                            "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"{}\" stroke-width=\"1.2\" stroke-opacity=\"0.85\" />\n",
                            sx(seg[0][0]),
                            sy(seg[0][1]),
                            sx(seg[1][0]),
                            sy(seg[1][1]),
                            overlay.color
                        ));
                    }
                }
            }
            OverlayKind::Histogram => {
                let bins = overlay.bins.max(2);
                let mut counts = vec![0usize; bins];
                for p in &opts {
                    let t = if (max_x - min_x).abs() < f64::EPSILON {
                        0.0
                    } else {
                        ((p[0] - min_x) / (max_x - min_x)).clamp(0.0, 1.0)
                    };
                    let b = ((t * (bins - 1) as f64).round() as usize).min(bins - 1);
                    counts[b] += 1;
                }
                let max_count = counts.iter().copied().max().unwrap_or(0) as f64;
                if max_count > 0.0 {
                    let mut path = String::new();
                    for (b, &c) in counts.iter().enumerate() {
                        let px = min_x + (max_x - min_x) * b as f64 / (bins - 1) as f64;
                        let py = origin_y - (c as f64 / max_count) * plot_h * 0.35;
                        if b == 0 {
                            path.push_str(&format!("M {:.2} {:.2}", sx(px), sy(py)));
                        } else {
                            path.push_str(&format!(" L {:.2} {:.2}", sx(px), sy(py)));
                        }
                    }
                    svg.push_str(&format!(
                        "<path d=\"{path}\" stroke=\"{}\" stroke-width=\"1.4\" fill=\"none\" stroke-opacity=\"0.9\" />\n",
                        overlay.color
                    ));
                }
            }
        }
    }

    // Gate overlays.
    for overlay in overlays {
        if overlay.points.len() < 2 {
            continue;
        }
        let mut path = String::new();
        for (k, p) in overlay.points.iter().enumerate() {
            let (Some(px), Some(py)) =
                (style.x_scale.transform(p[0]), style.y_scale.transform(p[1]))
            else {
                continue;
            };
            if k == 0 {
                path.push_str(&format!("M {:.2} {:.2}", sx(px), sy(py)));
            } else {
                path.push_str(&format!(" L {:.2} {:.2}", sx(px), sy(py)));
            }
        }
        if path.is_empty() {
            continue;
        }
        if overlay.closed {
            path.push_str(" Z");
        }
        let fill = match &overlay.fill {
            Some(f) => format!(" fill=\"{f}\" fill-opacity=\"0.2\""),
            None => " fill=\"none\"".to_string(),
        };
        let dash = if overlay.dashed {
            " stroke-dasharray=\"6 4\""
        } else {
            ""
        };
        svg.push_str(&format!(
            "<path d=\"{path}\" stroke=\"{}\" stroke-width=\"2\"{dash}{fill} />\n",
            overlay.stroke
        ));
    }

    // Population labels are drawn last so they sit above outlines.
    for overlay in overlays {
        let Some(label) = &overlay.label else {
            continue;
        };
        if label.text.is_empty() {
            continue;
        }
        let (Some(px), Some(py)) = (
            style.x_scale.transform(label.position[0]),
            style.y_scale.transform(label.position[1]),
        ) else {
            continue;
        };
        svg.push_str(&format!(
            "<text x=\"{:.2}\" y=\"{:.2}\" fill=\"{}\" font-size=\"{:.1}\" font-family=\"{}\" text-anchor=\"middle\">{}</text>\n",
            sx(px),
            sy(py),
            label.color,
            label.size,
            svg_escape(&style.label_font),
            svg_escape(&label.text)
        ));
    }

    // Legend for data overlays (top-right, below the plot title).
    if !data_overlays.is_empty() {
        let mut ly = 30.0;
        for overlay in data_overlays {
            if overlay.label.is_empty() {
                continue;
            }
            let lx = (style.width as f64 - 150.0).max(70.0);
            let label = if style.show_names_frequencies && !raw_x.is_empty() {
                format!(
                    "{} ({:.1}%)",
                    overlay.label,
                    overlay.points.len() as f64 / raw_x.len() as f64 * 100.0
                )
            } else {
                overlay.label.clone()
            };
            svg.push_str(&format!(
                "<rect x=\"{lx:.1}\" y=\"{ly:.1}\" width=\"10\" height=\"10\" fill=\"{}\" />\n",
                overlay.color
            ));
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" font-family=\"{}\" fill=\"{}\">{}</text>\n",
                lx + 14.0,
                ly + 9.0,
                svg_escape(&style.label_font),
                style.text_color,
                svg_escape(&label)
            ));
            ly += 14.0;
        }
    }

    // Density legend for map-based views (pseudocolour / zebra).
    if let Some((map, _bands)) = density {
        svg.push_str(&density_legend_svg(map, style.width as f32));
    }

    svg.push_str("</svg>\n");
    svg
}

/// Renders a spectral mixture-weight bar chart as SVG (C5 spectral plot /
/// population view): one bar per source, scaled by the largest weight.
pub fn spectral_weight_svg(weights: &[f32], width: u32, height: u32, color: &str) -> String {
    let max = weights.iter().cloned().fold(0.0_f32, f32::max);
    let scale = if max > 0.0 { height as f32 / max } else { 0.0 };
    let n = weights.len().max(1);
    let step = width as f32 / n as f32;
    let mut rects = String::new();
    for (i, w) in weights.iter().enumerate() {
        let h = (*w * scale).clamp(0.0, height as f32);
        let x = ((i as f32) * step).min((width as f32) - step).max(0.0);
        let y = height as f32 - h;
        rects.push_str(&format!(
            r#"<rect x="{x:.1}" y="{y:.1}" width="{step:.1}" height="{h:.1}" fill="{color}" />"#
        ));
    }
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><g>{rects}</g></svg>"#
    )
}

/// Renders a 1D histogram for a single parameter over a set of events.
///
/// `label` is the X-axis label; `y_label` is the Y-axis label (use `"Count"`
/// for the default). Axis-label font/rotation/size come from `style`.
#[allow(clippy::too_many_arguments)]
pub fn histogram_svg(
    data: &ndarray::Array2<f32>,
    param_idx: usize,
    label: &str,
    y_label: &str,
    indices: &[usize],
    value_range: Option<(f32, f32)>,
    bins: usize,
    style: &PlotStyle,
) -> String {
    let (_, n_params) = data.dim();
    assert!(param_idx < n_params);

    let sel: Vec<usize> = if indices.is_empty() {
        (0..data.nrows()).collect()
    } else {
        indices.to_vec()
    };

    let values: Vec<f32> = sel.iter().map(|&i| data[[i, param_idx]]).collect();
    let (min_v, max_v) = value_range.unwrap_or_else(|| {
        let (a, b) = compute_range(
            &values.iter().map(|v| *v as f64).collect::<Vec<_>>(),
            &style.x_scale,
        );
        (a as f32, b as f32)
    });
    let (min_v, max_v) = pad_range(min_v, max_v);
    let bin_count = bins.max(1);
    let bin_w = (max_v - min_v) / bin_count as f32;

    let mut counts = vec![0usize; bin_count];
    for &v in &values {
        if !v.is_finite() {
            continue;
        }
        let mut bi = ((v - min_v) / bin_w) as usize;
        if bi >= bin_count {
            bi = bin_count - 1;
        }
        counts[bi] += 1;
    }
    let counts = smooth_counts(&counts, style.smoothing);
    let max_count = counts.iter().max().copied().unwrap_or(1).max(1);

    let plot_w = style.width as f32 - 80.0;
    let plot_h = style.height as f32 - 80.0;
    let origin_x = 60.0;
    let origin_y = style.height as f32 - 40.0;

    let sx = |v: f32| origin_x + (v - min_v) / (max_v - min_v) * plot_w;
    let sy = |c: usize| origin_y - (c as f32 / max_count as f32) * plot_h;

    let mut svg = String::new();
    svg.push_str(&svg_header(style.width, style.height, &style.background));
    svg.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{}\" />\n",
        style.width, style.height, style.background
    ));

    // Axes.
    svg.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1\" />\n",
        origin_x,
        origin_y,
        origin_x + plot_w,
        origin_y,
        style.axis_color
    ));
    svg.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1\" />\n",
        origin_x,
        origin_y,
        origin_x,
        origin_y - plot_h,
        style.axis_color
    ));

    // Background grid (V5).
    if style.grid {
        for t in 0..=5 {
            let px = origin_x + plot_w * (t as f32 / 5.0);
            svg.push_str(&format!(
                "<line x1=\"{px:.2}\" y1=\"{:.2}\" x2=\"{px:.2}\" y2=\"{:.2}\" stroke=\"#e2e8f0\" stroke-width=\"0.6\" />\n",
                origin_y - plot_h,
                origin_y
            ));
        }
        for t in 0..=4 {
            let py = origin_y - plot_h * (t as f32 / 4.0);
            svg.push_str(&format!(
                "<line x1=\"{:.2}\" y1=\"{py:.2}\" x2=\"{:.2}\" y2=\"{py:.2}\" stroke=\"#e2e8f0\" stroke-width=\"0.6\" />\n",
                origin_x,
                origin_x + plot_w
            ));
        }
    }

    // Median line (V5).
    if style.medians {
        let raw: Vec<f64> = values.iter().map(|v| *v as f64).collect();
        if let Some(m) = median_of(&raw).filter(|v| *v >= min_v as f64 && *v <= max_v as f64) {
            svg.push_str(&format!(
                "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"{}\" stroke-width=\"1\" stroke-dasharray=\"5 4\" stroke-opacity=\"0.8\" />\n",
                sx(m as f32),
                origin_y - plot_h,
                sx(m as f32),
                origin_y,
                style.point_color
            ));
        }
    }

    // Bars. Inset each bar slightly from its neighbours so the histogram
    // renders with gaps instead of solid, blocky rectangles.
    for (b, &c) in counts.iter().enumerate() {
        let x0 = sx(min_v + b as f32 * bin_w);
        let x1 = sx(min_v + (b + 1) as f32 * bin_w);
        let y_top = sy(c);
        let bar_w = (x1 - x0).max(1.0) * 0.95;
        let bx0 = x0 + (x1 - x0 - bar_w) / 2.0;
        svg.push_str(&format!(
            "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"{}\" />\n",
            bx0,
            y_top,
            bar_w,
            origin_y - y_top,
            style.point_color
        ));
    }

    // Event count annotation (V5).
    if style.show_event_numbers {
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" font-family=\"{}\" fill=\"{}\" text-anchor=\"start\">n = {}</text>\n",
            origin_x + 6.0,
            origin_y - plot_h + 16.0,
            svg_escape(&style.label_font),
            style.text_color,
            values.len()
        ));
    }

    let y_center = origin_y - plot_h / 2.0;
    let x_anchor = (origin_x + plot_w / 2.0, style.height as f32 - 10.0);
    let y_anchor = ((origin_x as f64) - 30.0, y_center as f64);
    let x_text = label_with_scale(label, &style.x_scale, style.show_scale_labels);
    let y_text = if y_label.is_empty() { "Count" } else { y_label };
    if style.show_axis_labels {
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\" text-anchor=\"middle\"{}>{}</text>\n",
            x_anchor.0,
            x_anchor.1,
            svg_escape(&style.x_label_font),
            style.x_label_size,
            style.text_color,
            rotation_attr(style.x_label_rotation, (x_anchor.0 as f64, x_anchor.1 as f64)),
            svg_escape(&x_text)
        ));
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\" text-anchor=\"middle\"{}>{}</text>\n",
            y_anchor.0,
            y_anchor.1,
            svg_escape(&style.y_label_font),
            style.y_label_size,
            style.text_color,
            rotation_attr(style.y_label_rotation, y_anchor),
            svg_escape(y_text)
        ));
    }

    // Tick labels and marks.
    let xticks = 5;
    for t in 0..=xticks {
        let v = min_v + (max_v - min_v) * (t as f32 / xticks as f32);
        let px = sx(v);
        if style.show_numbers {
            svg.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-size=\"10\" fill=\"{}\" text-anchor=\"middle\">{}</text>\n",
                px,
                origin_y + 15.0,
                style.text_color,
                svg_escape(&format_value_with(v as f64, style.notation))
            ));
        }
        if style.show_ticks {
            svg.push_str(&format!(
                "<line x1=\"{px:.2}\" y1=\"{:.2}\" x2=\"{px:.2}\" y2=\"{:.2}\" stroke=\"{}\" stroke-width=\"0.5\" />\n",
                origin_y,
                origin_y - 5.0,
                style.axis_color
            ));
        }
    }

    svg.push_str("</svg>\n");
    svg
}

// ---------------------------------------------------------------------------
// Density / contour analysis
// ---------------------------------------------------------------------------

/// A regular 2D grid of event counts, used for density colouring and contours.
#[derive(Debug, Clone)]
pub struct DensityGrid {
    pub nx: usize,
    pub ny: usize,
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
    /// Row-major densities, index `j * nx + i` (i over x, j over y).
    pub values: Vec<f64>,
}

impl DensityGrid {
    /// Value at grid node `(i, j)`, clamped to the grid extents.
    pub fn at(&self, i: usize, j: usize) -> f64 {
        let i = i.min(self.nx.saturating_sub(1));
        let j = j.min(self.ny.saturating_sub(1));
        self.values[j * self.nx + i]
    }

    pub fn max(&self) -> f64 {
        self.values.iter().copied().fold(0.0, f64::max)
    }

    fn x_at(&self, i: usize) -> f64 {
        if self.nx <= 1 {
            self.x0
        } else {
            self.x0 + (self.x1 - self.x0) * i as f64 / (self.nx - 1) as f64
        }
    }

    fn y_at(&self, j: usize) -> f64 {
        if self.ny <= 1 {
            self.y0
        } else {
            self.y0 + (self.y1 - self.y0) * j as f64 / (self.ny - 1) as f64
        }
    }
}

/// Build a histogram density grid over the given rectangle.
pub fn density_grid(
    points: &[[f64; 2]],
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    nx: usize,
    ny: usize,
) -> DensityGrid {
    let nx = nx.max(2);
    let ny = ny.max(2);
    let w = if (x1 - x0).abs() < f64::EPSILON {
        1.0
    } else {
        x1 - x0
    };
    let h = if (y1 - y0).abs() < f64::EPSILON {
        1.0
    } else {
        y1 - y0
    };
    let mut values = vec![0.0f64; nx * ny];
    for p in points {
        if !p[0].is_finite() || !p[1].is_finite() {
            continue;
        }
        let fx = (p[0] - x0) / w * (nx - 1) as f64;
        let fy = (p[1] - y0) / h * (ny - 1) as f64;
        let i = fx.round().clamp(0.0, (nx - 1) as f64) as usize;
        let j = fy.round().clamp(0.0, (ny - 1) as f64) as usize;
        values[j * nx + i] += 1.0;
    }
    DensityGrid {
        nx,
        ny,
        x0,
        x1: x0 + w,
        y0,
        y1: y0 + h,
        values,
    }
}

/// Bilinearly sample the density at each point (used for gradient colouring).
pub fn point_densities(points: &[[f64; 2]], grid: &DensityGrid) -> Vec<f64> {
    let w = if (grid.x1 - grid.x0).abs() < f64::EPSILON {
        1.0
    } else {
        grid.x1 - grid.x0
    };
    let h = if (grid.y1 - grid.y0).abs() < f64::EPSILON {
        1.0
    } else {
        grid.y1 - grid.y0
    };
    points
        .iter()
        .map(|p| {
            let fx = (p[0] - grid.x0) / w * (grid.nx - 1) as f64;
            let fy = (p[1] - grid.y0) / h * (grid.ny - 1) as f64;
            let i = fx.floor();
            let j = fy.floor();
            let tx = fx - i;
            let ty = fy - j;
            let i0 = i.clamp(0.0, (grid.nx - 2) as f64) as usize;
            let j0 = j.clamp(0.0, (grid.ny - 2) as f64) as usize;
            let v00 = grid.at(i0, j0);
            let v10 = grid.at(i0 + 1, j0);
            let v01 = grid.at(i0, j0 + 1);
            let v11 = grid.at(i0 + 1, j0 + 1);
            let a = v00 * (1.0 - tx) + v10 * tx;
            let b = v01 * (1.0 - ty) + v11 * ty;
            a * (1.0 - ty) + b * ty
        })
        .collect()
}

/// Applies weighted 3x3 blur passes to a density grid (V5 smoothing).
///
/// `smoothing` is a fraction in `[0, 1]`; each 0.25 adds one pass.
pub fn smooth_density_grid(grid: &mut DensityGrid, smoothing: f32) {
    let passes = (smoothing.clamp(0.0, 1.0) * 4.0).round() as usize;
    if passes == 0 || grid.nx < 3 || grid.ny < 3 {
        return;
    }
    for _ in 0..passes {
        let src = grid.values.clone();
        for j in 0..grid.ny {
            for i in 0..grid.nx {
                let mut sum = 0.0;
                let mut weight = 0.0;
                for dj in -1i32..=1 {
                    for di in -1i32..=1 {
                        let ii = i as i32 + di;
                        let jj = j as i32 + dj;
                        if ii < 0 || jj < 0 || ii >= grid.nx as i32 || jj >= grid.ny as i32 {
                            continue;
                        }
                        let w = match (di, dj) {
                            (0, 0) => 4.0,
                            (0, _) | (_, 0) => 2.0,
                            _ => 1.0,
                        };
                        sum += src[jj as usize * grid.nx + ii as usize] * w;
                        weight += w;
                    }
                }
                grid.values[j * grid.nx + i] = sum / weight;
            }
        }
    }
}

/// Density value at the `(100 - clip_percent)`th percentile.
///
/// Outlier clipping saturates the colour/contour scale at this value so a few
/// extremely dense cells do not wash out the rest of the plot (V5). `0`
/// disables clipping and returns the true maximum.
pub fn clipped_density_max(values: &[f64], clip_percent: f64) -> f64 {
    let mut vals: Vec<f64> = values
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    if vals.is_empty() {
        return 0.0;
    }
    let pct = clip_percent.clamp(0.0, 50.0);
    if pct <= 0.0 {
        return vals.iter().copied().fold(0.0, f64::max);
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx =
        (((1.0 - pct / 100.0) * (vals.len() - 1) as f64).round() as usize).min(vals.len() - 1);
    vals[idx].max(f64::MIN_POSITIVE)
}

/// Median of the finite values (used by the V5 median overlays).
pub fn median_of(values: &[f64]) -> Option<f64> {
    let mut vals: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if vals.is_empty() {
        return None;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = vals.len() / 2;
    if vals.len().is_multiple_of(2) {
        Some((vals[mid - 1] + vals[mid]) / 2.0)
    } else {
        Some(vals[mid])
    }
}

/// Counts values in `bins` equal-width buckets spanning `[lo, hi]` (V6).
pub fn axis_histogram_counts(values: &[f64], lo: f64, hi: f64, bins: usize) -> Vec<usize> {
    let bins = bins.max(1);
    let mut counts = vec![0usize; bins];
    let span = hi - lo;
    if !span.is_finite() || span.abs() < f64::EPSILON {
        return counts;
    }
    for &v in values {
        if !v.is_finite() || v < lo || v > hi {
            continue;
        }
        let t = ((v - lo) / span).clamp(0.0, 1.0);
        let b = ((t * bins as f64).floor() as usize).min(bins - 1);
        counts[b] += 1;
    }
    counts
}

/// Smooths histogram bin counts with a weighted `[1, 2, 1]` moving average.
pub fn smooth_counts(counts: &[usize], smoothing: f32) -> Vec<usize> {
    let passes = (smoothing.clamp(0.0, 1.0) * 3.0).round() as usize;
    if passes == 0 || counts.len() < 3 {
        return counts.to_vec();
    }
    let mut current = counts.to_vec();
    for _ in 0..passes {
        let src = current.clone();
        for i in 0..src.len() {
            let prev = src[i.saturating_sub(1)] as f64;
            let next = src[(i + 1).min(src.len() - 1)] as f64;
            let blended = (prev + 2.0 * src[i] as f64 + next) / 4.0;
            current[i] = blended.round() as usize;
        }
    }
    current
}

/// Choose `n` evenly spaced contour levels between 0 and `max`.
pub fn contour_levels(max: f64, n: usize) -> Vec<f64> {
    let n = n.max(1);
    (1..=n).map(|k| max * k as f64 / (n + 1) as f64).collect()
}

/// Extract straight contour segments at `level` using marching squares.
///
/// Each returned segment is a pair of interpolated endpoints in the grid's
/// coordinate space.
pub fn contour_segments(grid: &DensityGrid, level: f64) -> Vec<[[f64; 2]; 2]> {
    let mut segments = Vec::new();
    for j in 0..grid.ny.saturating_sub(1) {
        for i in 0..grid.nx.saturating_sub(1) {
            let v00 = grid.at(i, j);
            let v10 = grid.at(i + 1, j);
            let v11 = grid.at(i + 1, j + 1);
            let v01 = grid.at(i, j + 1);

            let x0 = grid.x_at(i);
            let x1 = grid.x_at(i + 1);
            let y0 = grid.y_at(j);
            let y1 = grid.y_at(j + 1);

            let idx = (v00 > level) as u8
                | ((v10 > level) as u8) << 1
                | ((v11 > level) as u8) << 2
                | ((v01 > level) as u8) << 3;
            if idx == 0 || idx == 15 {
                continue;
            }

            let interp = |a: f64, b: f64| -> f64 {
                if (b - a).abs() < f64::EPSILON {
                    0.5
                } else {
                    ((level - a) / (b - a)).clamp(0.0, 1.0)
                }
            };
            // Edge midpoints (edge ids: 0 bottom, 1 right, 2 top, 3 left).
            let edge = |e: u8| -> [f64; 2] {
                match e {
                    0 => {
                        let t = interp(v00, v10);
                        [x0 + (x1 - x0) * t, y0]
                    }
                    1 => {
                        let t = interp(v10, v11);
                        [x1, y0 + (y1 - y0) * t]
                    }
                    2 => {
                        let t = interp(v01, v11);
                        [x0 + (x1 - x0) * t, y1]
                    }
                    _ => {
                        let t = interp(v00, v01);
                        [x0, y0 + (y1 - y0) * t]
                    }
                }
            };

            let pairs: &[(u8, u8)] = match idx {
                1 => &[(3, 0)],
                2 => &[(0, 1)],
                3 => &[(3, 1)],
                4 => &[(1, 2)],
                5 => &[(3, 0), (1, 2)],
                6 => &[(0, 2)],
                7 => &[(3, 2)],
                8 => &[(3, 2)],
                9 => &[(0, 2)],
                10 => &[(0, 1), (2, 3)],
                11 => &[(1, 2)],
                12 => &[(3, 1)],
                13 => &[(0, 1)],
                14 => &[(3, 0)],
                _ => &[],
            };
            for (a, b) in pairs {
                segments.push([edge(*a), edge(*b)]);
            }
        }
    }
    segments
}

/// Maps a normalized density `t` in `[0, 1]` to an RGB triple (blue -> cyan ->
/// yellow -> red). Shared by the GUI and SVG renderers so both look identical.
pub fn density_rgb(t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    // Piecewise-linear colour ramp.
    let stops: [(f64, (f64, f64, f64)); 4] = [
        (0.0, (0.0, 0.0, 140.0)),
        (0.35, (0.0, 190.0, 220.0)),
        (0.7, (250.0, 220.0, 40.0)),
        (1.0, (220.0, 20.0, 20.0)),
    ];
    for w in stops.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t >= t0 && t <= t1 {
            let f = if (t1 - t0).abs() < f64::EPSILON {
                0.0
            } else {
                (t - t0) / (t1 - t0)
            };
            let r = c0.0 + (c1.0 - c0.0) * f;
            let g = c0.1 + (c1.1 - c0.1) * f;
            let b = c0.2 + (c1.2 - c0.2) * f;
            return (r.round() as u8, g.round() as u8, b.round() as u8);
        }
    }
    (220, 20, 20)
}

/// Converts a normalized density to a `#rrggbb` string.
pub fn density_hex(t: f64) -> String {
    let (r, g, b) = density_rgb(t);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Parses a `#rrggbb` (or `#rgb`) colour into an RGB triple.
pub fn parse_hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().trim_start_matches('#');
    match s.len() {
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some((r, g, b))
        }
        3 => {
            let r = u8::from_str_radix(&s[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&s[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&s[2..3], 16).ok()? * 17;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn compute_range(values: &[f64], scale: &AxisScale) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for &v in values {
        if !v.is_finite() || (scale.positive_only() && v <= 0.0) {
            continue;
        }
        min = min.min(v);
        max = max.max(v);
    }
    if !min.is_finite() {
        (1.0, 10.0)
    } else if (min - max).abs() < f64::EPSILON {
        if min.abs() > 1.0 {
            (min * 0.5, min * 1.5)
        } else {
            (min - 1.0, max + 1.0)
        }
    } else {
        (min, max)
    }
}

fn pad_span(min: f64, max: f64) -> (f64, f64) {
    if !min.is_finite() || !max.is_finite() || (max - min).abs() < f64::EPSILON {
        return (-1.0, 1.0);
    }
    let pad = (max - min) * 0.05;
    (min - pad, max + pad)
}

fn pad_range(min: f32, max: f32) -> (f32, f32) {
    let span = max - min;
    let pad = if span.abs() < f32::EPSILON {
        1.0
    } else {
        span * 0.05
    };
    (min - pad, max + pad)
}

/// Renders a dot plot SVG whose points are coloured by a density colour map
/// (pseudocolour / zebra), using the supplied number of zebra bands.
#[allow(clippy::too_many_arguments)]
pub fn dot_plot_svg_density(
    data: &ndarray::Array2<f32>,
    x_idx: usize,
    y_idx: usize,
    x_label: &str,
    y_label: &str,
    indices: &[usize],
    x_range: Option<(f32, f32)>,
    y_range: Option<(f32, f32)>,
    overlays: &[PlotOverlay],
    style: &PlotStyle,
    density_map: &DensityMap,
    bands: usize,
) -> String {
    dot_plot_svg_impl(
        data,
        x_idx,
        y_idx,
        x_label,
        y_label,
        indices,
        x_range,
        y_range,
        overlays,
        &[],
        style,
        Some((density_map, bands)),
    )
}

/// Renders a dot plot SVG (density-based point colours only when `style.gradient`
/// is set, using the built-in rainbow ramp).
#[allow(clippy::too_many_arguments)]
pub fn dot_plot_svg(
    data: &ndarray::Array2<f32>,
    x_idx: usize,
    y_idx: usize,
    x_label: &str,
    y_label: &str,
    indices: &[usize],
    x_range: Option<(f32, f32)>,
    y_range: Option<(f32, f32)>,
    overlays: &[PlotOverlay],
    style: &PlotStyle,
) -> String {
    dot_plot_svg_impl(
        data,
        x_idx,
        y_idx,
        x_label,
        y_label,
        indices,
        x_range,
        y_range,
        overlays,
        &[],
        style,
        None,
    )
}

/// Renders a plot with data overlays (extra samples/populations) and gate
/// outlines on shared axes. `density` optionally colours the base events with
/// a density map (pseudocolour / zebra).
#[allow(clippy::too_many_arguments)]
pub fn dot_plot_svg_multi(
    data: &ndarray::Array2<f32>,
    x_idx: usize,
    y_idx: usize,
    x_label: &str,
    y_label: &str,
    indices: &[usize],
    x_range: Option<(f32, f32)>,
    y_range: Option<(f32, f32)>,
    overlays: &[PlotOverlay],
    data_overlays: &[DataOverlay],
    style: &PlotStyle,
    density: Option<(&DensityMap, usize)>,
) -> String {
    dot_plot_svg_impl(
        data,
        x_idx,
        y_idx,
        x_label,
        y_label,
        indices,
        x_range,
        y_range,
        overlays,
        data_overlays,
        style,
        density,
    )
}

fn svg_header(width: u32, height: u32, background: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" style="background:{background}">"#
    )
}

/// Layout mode for a multigraph sheet (V2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultigraphMode {
    /// One row: the current X parameter against every parameter as Y.
    AllByX,
    /// One column: every parameter as X against the current Y parameter.
    AllByY,
    /// Square matrix of every ordered parameter pair (diagonal omitted).
    NxN,
}

/// Panel assignments `(x_idx, y_idx)` for a multigraph sheet.
pub fn multigraph_panels(
    n_params: usize,
    x_idx: usize,
    y_idx: usize,
    mode: MultigraphMode,
) -> Vec<(usize, usize)> {
    match mode {
        MultigraphMode::AllByX => (0..n_params)
            .map(|j| (x_idx.min(n_params.saturating_sub(1)), j))
            .collect(),
        MultigraphMode::AllByY => (0..n_params)
            .map(|i| (i, y_idx.min(n_params.saturating_sub(1))))
            .collect(),
        MultigraphMode::NxN => {
            let mut panels = Vec::new();
            for y in 0..n_params {
                for x in 0..n_params {
                    if x != y {
                        panels.push((x, y));
                    }
                }
            }
            panels
        }
    }
}

/// Renders a grid of dot plots as a single SVG sheet.
///
/// `labels` supplies the display name for each parameter index; `columns`
/// controls how many panels are placed per row. Each panel is rendered by
/// [`dot_plot_svg`] using the scale settings in `style`.
pub fn multigraph_svg(
    data: &ndarray::Array2<f32>,
    panels: &[(usize, usize)],
    columns: usize,
    panel_width: u32,
    panel_height: u32,
    labels: &[String],
    style: &PlotStyle,
) -> String {
    let columns = columns.max(1);
    let rows = panels.len().div_ceil(columns);
    let gap = 8u32;
    let total_w = columns as u32 * panel_width + (columns as u32 + 1) * gap;
    let total_h = rows as u32 * panel_height + (rows as u32 + 1) * gap;
    let mut svg = svg_header(total_w, total_h, &style.background);
    svg.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{total_w}\" height=\"{total_h}\" fill=\"{}\" />\n",
        style.background
    ));
    for (k, &(x, y)) in panels.iter().enumerate() {
        let col = k % columns;
        let row = k / columns;
        let px = gap + col as u32 * (panel_width + gap);
        let py = gap + row as u32 * (panel_height + gap);
        let mut panel_style = style.clone();
        panel_style.width = panel_width;
        panel_style.height = panel_height;
        panel_style.title.clear();
        let x_label = labels.get(x).cloned().unwrap_or_default();
        let y_label = labels.get(y).cloned().unwrap_or_default();
        let panel = dot_plot_svg(
            data,
            x,
            y,
            &x_label,
            &y_label,
            &[],
            None,
            None,
            &[],
            &panel_style,
        );
        svg.push_str(&format!(
            "<g transform=\"translate({px} {py})\">{panel}</g>\n"
        ));
    }
    svg.push_str("</svg>\n");
    svg
}

/// Renders a grid of dot plots with data overlays and optional density
/// colouring on every panel (L5 multigraph/NxN figures).
#[allow(clippy::too_many_arguments)]
pub fn multigraph_svg_multi(
    data: &ndarray::Array2<f32>,
    panels: &[(usize, usize)],
    columns: usize,
    panel_width: u32,
    panel_height: u32,
    labels: &[String],
    style: &PlotStyle,
    overlays: &[PlotOverlay],
    data_overlays: &[DataOverlay],
    density: Option<&DensityMap>,
) -> String {
    let columns = columns.max(1);
    let rows = panels.len().div_ceil(columns);
    let gap = 8u32;
    let total_w = columns as u32 * panel_width + (columns as u32 + 1) * gap;
    let total_h = rows as u32 * panel_height + (rows as u32 + 1) * gap;
    let mut svg = svg_header(total_w, total_h, &style.background);
    svg.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{total_w}\" height=\"{total_h}\" fill=\"{}\" />\n",
        style.background
    ));
    for (k, &(x, y)) in panels.iter().enumerate() {
        let col = k % columns;
        let row = k / columns;
        let px = gap + col as u32 * (panel_width + gap);
        let py = gap + row as u32 * (panel_height + gap);
        let mut panel_style = style.clone();
        panel_style.width = panel_width;
        panel_style.height = panel_height;
        panel_style.title.clear();
        let x_label = labels.get(x).cloned().unwrap_or_default();
        let y_label = labels.get(y).cloned().unwrap_or_default();
        let panel = dot_plot_svg_multi(
            data,
            x,
            y,
            &x_label,
            &y_label,
            &[],
            None,
            None,
            overlays,
            data_overlays,
            &panel_style,
            density.map(|map| (map, panel_style.resolution)),
        );
        svg.push_str(&format!(
            "<g transform=\"translate({px} {py})\">{panel}</g>\n"
        ));
    }
    svg.push_str("</svg>\n");
    svg
}

/// Render an NxN square matrix of every ordered parameter pair (L5).
#[allow(clippy::too_many_arguments)]
pub fn nxn_svg(
    data: &ndarray::Array2<f32>,
    columns: usize,
    panel_width: u32,
    panel_height: u32,
    labels: &[String],
    style: &PlotStyle,
) -> String {
    let n = labels.len().max(data.ncols());
    let panels = multigraph_panels(n, 0, 0, MultigraphMode::NxN);
    multigraph_svg(
        data,
        &panels,
        columns,
        panel_width,
        panel_height,
        labels,
        style,
    )
}

/// Render an NxN matrix with data overlays and optional density colouring
/// (L5).
#[allow(clippy::too_many_arguments)]
pub fn nxn_svg_multi(
    data: &ndarray::Array2<f32>,
    columns: usize,
    panel_width: u32,
    panel_height: u32,
    labels: &[String],
    style: &PlotStyle,
    overlays: &[PlotOverlay],
    data_overlays: &[DataOverlay],
    density: Option<&DensityMap>,
) -> String {
    let n = labels.len().max(data.ncols());
    let panels = multigraph_panels(n, 0, 0, MultigraphMode::NxN);
    multigraph_svg_multi(
        data,
        &panels,
        columns,
        panel_width,
        panel_height,
        labels,
        style,
        overlays,
        data_overlays,
        density,
    )
}

/// Built-in per-figure display preset (L5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlotPreset {
    Dots,
    Pseudocolor,
    Contour,
    Histogram,
    Zebra,
}

impl PlotPreset {
    /// All built-in presets, in display order.
    pub fn all() -> [Self; 5] {
        [
            Self::Dots,
            Self::Pseudocolor,
            Self::Contour,
            Self::Histogram,
            Self::Zebra,
        ]
    }

    /// Stable identifier.
    pub fn id(self) -> &'static str {
        match self {
            Self::Dots => "dots",
            Self::Pseudocolor => "pseudocolor",
            Self::Contour => "contour",
            Self::Histogram => "histogram",
            Self::Zebra => "zebra",
        }
    }

    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Dots => "Dot plot",
            Self::Pseudocolor => "Pseudocolor",
            Self::Contour => "Contour",
            Self::Histogram => "Histogram",
            Self::Zebra => "Zebra",
        }
    }

    /// Apply the preset's display mode to a style, keeping size and colours.
    pub fn apply(self, style: &PlotStyle) -> PlotStyle {
        let mut style = style.clone();
        match self {
            Self::Dots => {
                style.gradient = false;
                style.contour = false;
                style.point_opacity = 0.6;
                style.large_dots = false;
            }
            Self::Pseudocolor => {
                style.gradient = true;
                style.contour = false;
                style.point_opacity = 1.0;
            }
            Self::Contour => {
                style.gradient = false;
                style.contour = true;
            }
            Self::Histogram => {
                style.gradient = false;
                style.contour = false;
                style.point_opacity = 0.8;
            }
            Self::Zebra => {
                style.gradient = true;
                style.contour = false;
                style.point_opacity = 1.0;
                style.large_dots = true;
            }
        }
        style
    }
}

/// A named per-figure preset: style plus optional density map and multigraph
/// mode (L5).
#[derive(Debug, Clone, PartialEq)]
pub struct FigurePreset {
    pub name: String,
    pub style: PlotStyle,
    pub density_map: Option<DensityMap>,
    pub mode: Option<MultigraphMode>,
}

impl FigurePreset {
    /// Create a preset from an explicit style.
    pub fn new(name: impl Into<String>, style: PlotStyle) -> Self {
        Self {
            name: name.into(),
            style,
            density_map: None,
            mode: None,
        }
    }

    /// Create a preset by applying a built-in display preset to `base`.
    pub fn from_plot_preset(name: impl Into<String>, preset: PlotPreset, base: &PlotStyle) -> Self {
        Self::new(name, preset.apply(base))
    }

    /// The built-in preset collection.
    pub fn built_in() -> Vec<Self> {
        let base = PlotStyle::default();
        let mut presets: Vec<Self> = PlotPreset::all()
            .into_iter()
            .map(|preset| {
                Self::from_plot_preset(preset.id(), preset, &base).with_density_map(
                    if preset == PlotPreset::Zebra {
                        DensityMap::Zebra
                    } else {
                        DensityMap::Viridis
                    },
                )
            })
            .collect();
        for preset in &mut presets {
            if preset.name == "contour" || preset.name == "histogram" {
                preset.density_map = None;
            }
        }
        presets
    }

    /// Attach a density map (used by pseudocolour/zebra figures).
    pub fn with_density_map(mut self, map: DensityMap) -> Self {
        self.density_map = Some(map);
        self
    }

    /// Attach a multigraph layout mode.
    pub fn with_mode(mut self, mode: MultigraphMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Apply this preset to a target figure style, preserving the target's
    /// dimensions, background, fonts and title.
    pub fn apply(&self, target: &PlotStyle) -> PlotStyle {
        let mut style = self.style.clone();
        style.width = target.width;
        style.height = target.height;
        style.background = target.background.clone();
        style.title = target.title.clone();
        style.label_font = target.label_font.clone();
        style.x_label_font = target.x_label_font.clone();
        style.y_label_font = target.y_label_font.clone();
        style.title_size = target.title_size;
        style
    }
}

/// Library of named per-figure presets with replace-by-name semantics (L5).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FigurePresetRegistry {
    pub presets: Vec<FigurePreset>,
}

impl FigurePresetRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry preloaded with the built-in presets.
    pub fn with_built_ins() -> Self {
        Self {
            presets: FigurePreset::built_in(),
        }
    }

    /// Save a preset, replacing any existing preset with the same name.
    pub fn save(&mut self, preset: FigurePreset) -> &mut Self {
        if let Some(existing) = self
            .presets
            .iter_mut()
            .find(|existing| existing.name == preset.name)
        {
            *existing = preset;
        } else {
            self.presets.push(preset);
        }
        self
    }

    pub fn get(&self, name: &str) -> Option<&FigurePreset> {
        self.presets.iter().find(|preset| preset.name == name)
    }

    pub fn names(&self) -> Vec<&str> {
        self.presets
            .iter()
            .map(|preset| preset.name.as_str())
            .collect()
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.presets.len();
        self.presets.retain(|preset| preset.name != name);
        self.presets.len() != before
    }

    /// Apply a named preset to a figure style.
    pub fn apply(&self, name: &str, target: &PlotStyle) -> Option<PlotStyle> {
        self.get(name).map(|preset| preset.apply(target))
    }
}

/// Render a line plot from table points (x, y) as SVG (TB5 table plots: line
/// plot of a table's two columns).
pub fn table_line_plot_svg(points: &[(f64, f64)], width: u32, height: u32) -> String {
    if points.is_empty() {
        return format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"></svg>"#
        );
    }
    let xs: Vec<f64> = points.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = points.iter().map(|p| p.1).collect();
    let min_x = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_x = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min_y = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_y = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let x_span = (max_x - min_x).max(f64::EPSILON);
    let y_span = (max_y - min_y).max(f64::EPSILON);
    let x = |v: f64| 10.0 + 80.0 * ((v - min_x) / x_span);
    let y = |v: f64| 10.0 + 80.0 * (1.0 - (v - min_y) / y_span);
    let coords: Vec<String> = points
        .iter()
        .map(|(x0, y0)| format!("{:.1},{:.1}", x(*x0), y(*y0)))
        .collect();
    let points_attr = coords.join(" ");
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><polyline points="{points_attr}" fill="none" stroke="#000000" /></svg>"##
    )
}

/// Result of fitting a straight line to table plot points (TB5 table plots:
/// correlation/regression).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegressionFit {
    /// Slope of the fitted line.
    pub slope: f64,
    /// Intercept of the fitted line.
    pub intercept: f64,
    /// Coefficient of determination (r²) of the fit; `1.0` is a perfect fit.
    pub r_squared: f64,
}

/// Fit `y = slope * x + intercept` to table plot points by least squares and
/// report the slope, intercept and coefficient of determination (TB5 table
/// plots: correlation/regression).
pub fn linear_regression(points: &[(f64, f64)]) -> Option<RegressionFit> {
    if points.len() < 2 {
        return None;
    }
    let n = points.len() as f64;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    for &(x, y) in points {
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-12 {
        // All x values identical: no function fit is possible.
        return None;
    }
    let slope = (n * sxy - sx * sy) / denom;
    let intercept = (sy - slope * sx) / n;
    let mean_y = sy / n;
    let mut ss_res = 0.0;
    for &(x, y) in points {
        let pred = slope * x + intercept;
        let d = y - pred;
        ss_res += d * d;
    }
    let mut ss_tot = 0.0;
    for &(_, y) in points {
        let d = y - mean_y;
        ss_tot += d * d;
    }
    let r_squared = if ss_tot > 0.0 {
        1.0 - ss_res / ss_tot
    } else {
        0.0
    };
    Some(RegressionFit {
        slope,
        intercept,
        r_squared,
    })
}

/// Data-space bounds (min_x, max_x, min_y, max_y) of a table plot series.
fn table_plot_bounds(points: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let xs: Vec<f64> = points.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = points.iter().map(|p| p.1).collect();
    (
        xs.iter().cloned().fold(f64::INFINITY, f64::min),
        xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        ys.iter().cloned().fold(f64::INFINITY, f64::min),
        ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    )
}

/// Map data-space coordinates into SVG plot coordinates (10px padding, 80x80 plot area).
fn table_plot_xy(min_x: f64, max_x: f64, min_y: f64, max_y: f64, x: f64, y: f64) -> (f64, f64) {
    let x_span = (max_x - min_x).max(f64::EPSILON);
    let y_span = (max_y - min_y).max(f64::EPSILON);
    (
        10.0 + 80.0 * ((x - min_x) / x_span),
        10.0 + 80.0 * (1.0 - (y - min_y) / y_span),
    )
}

/// Point string `"px,py"` for a table plot coordinate.
fn table_plot_pt(min_x: f64, max_x: f64, min_y: f64, max_y: f64, x: f64, y: f64) -> String {
    let (px, py) = table_plot_xy(min_x, max_x, min_y, max_y, x, y);
    format!("{px:.1},{py:.1}")
}

/// Empty-SVG template for table plots with no data.
fn table_plot_svg_empty(width: u32, height: u32) -> String {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"></svg>"#)
}

/// Render a regression / correlation table plot as SVG (TB5 table plots: line
/// plus fitted regression line, annotated with the r² of the fit).
pub fn table_regression_svg(points: &[(f64, f64)], width: u32, height: u32) -> String {
    if points.is_empty() {
        return table_plot_svg_empty(width, height);
    }
    let (min_x, max_x, min_y, max_y) = table_plot_bounds(points);
    let data_coords: Vec<String> = points
        .iter()
        .map(|&(x, y)| table_plot_pt(min_x, max_x, min_y, max_y, x, y))
        .collect();
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\"><polyline points=\"{}\" fill=\"none\" stroke=\"#000000\" /></svg>",
        data_coords.join(" ")
    );
    if let Some(fit) = linear_regression(points) {
        let (x0, y0) = table_plot_xy(
            min_x,
            max_x,
            min_y,
            max_y,
            min_x,
            fit.slope * min_x + fit.intercept,
        );
        let line = format!(
            "<polyline points=\"{0},{1}\" fill=\"none\" stroke=\"#d43\" /><text x=\"{2:.1}\" y=\"{3:.1}\" font-size=\"8\" fill=\"#d43\">r^2={4:.3}</text>",
            table_plot_pt(
                min_x,
                max_x,
                min_y,
                max_y,
                min_x,
                fit.slope * min_x + fit.intercept
            ),
            table_plot_pt(
                min_x,
                max_x,
                min_y,
                max_y,
                max_x,
                fit.slope * max_x + fit.intercept
            ),
            x0,
            y0,
            fit.r_squared
        );
        svg = svg.replace("</svg>", &line);
    }
    svg
}

/// Quartiles (Q1, median, Q3) of a value set via linear interpolation (TB5 table
/// plots: box-and-whisker).
pub fn quartiles(values: &[f64]) -> (f64, f64, f64) {
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    if n == 0 {
        return (0.0, 0.0, 0.0);
    }
    if n == 1 {
        return (sorted[0], sorted[0], sorted[0]);
    }
    let at = |p: f64| -> f64 {
        let h = (n as f64 - 1.0) * p;
        let lo = h.floor() as usize;
        let hi = (lo + 1).min(n - 1);
        let frac = h - lo as f64;
        sorted[lo] + (sorted[hi] - sorted[lo]) * frac
    };
    (at(0.25), at(0.5), at(0.75))
}

/// Render a box-and-whisker table plot as SVG (TB5 table plots: box-and-whisker).
pub fn table_box_whisker_svg(values: &[f64], width: u32, height: u32) -> String {
    if values.is_empty() {
        return table_plot_svg_empty(width, height);
    }
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let (q1, median, q3) = quartiles(values);
    let span = (max - min).max(f64::EPSILON);
    let qx = |v: f64| 10.0 + 80.0 * ((v - min) / span);
    let qy = |v: f64| 10.0 + 80.0 * (1.0 - ((v - min) / span));
    let box_x = qx(q1);
    let box_w = qx(q3) - qx(q1);
    let box_y = qy(q3);
    let box_h = qy(q1) - qy(q3);
    let x_min = qx(min);
    let x_q1 = qx(q1);
    let x_q3 = qx(q3);
    let x_max = qx(max);
    let y_max = qy(max);
    let y_min = qy(min);
    let y_q1 = qy(q1);
    let y_q3 = qy(q3);
    let y_median = qy(median);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\"><rect x=\"{box_x:.1}\" y=\"{box_y:.1}\" width=\"{box_w:.1}\" height=\"{box_h:.1}\" fill=\"none\" stroke=\"#000000\" /><line x1=\"{x_min:.1}\" y1=\"{y_min:.1}\" x2=\"{x_q1:.1}\" y2=\"{y_q1:.1}\" stroke=\"#000000\" /><line x1=\"{x_q3:.1}\" y1=\"{y_q3:.1}\" x2=\"{x_max:.1}\" y2=\"{y_max:.1}\" stroke=\"#000000\" /><line x1=\"{x_q1:.1}\" y1=\"{y_median:.1}\" x2=\"{x_q3:.1}\" y2=\"{y_median:.1}\" stroke=\"#000000\" /></svg>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn format_tick_uses_compact_scientific_notation() {
        // Regression: the exponent marker used to be duplicated ("7eee4").
        assert_eq!(format_tick(70000.0, &AxisScale::Linear), "7e4");
        assert_eq!(format_tick(1_000_000.0, &AxisScale::Linear), "1e6");
        assert_eq!(format_tick(1e7, &AxisScale::Linear), "1e7");
        assert_eq!(format_tick(5_300_871.0, &AxisScale::Linear), "5.3e6");
        assert_eq!(format_tick(-70000.0, &AxisScale::Linear), "-7e4");
        assert_eq!(format_tick(1e-3, &AxisScale::Linear), "1e-3");
        // Log scale receives the exponent and raises it back.
        assert_eq!(format_tick(6.0, &AxisScale::Log), "1e6");
        // Small / mid values stay in plain notation.
        assert_eq!(format_tick(123.0, &AxisScale::Linear), "123");
        assert_eq!(format_tick(1.5, &AxisScale::Linear), "1.50");
    }

    #[test]
    fn dot_plot_svg_renders() {
        let data = array![[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]];
        let svg = dot_plot_svg(
            &data,
            0,
            1,
            "FSC",
            "SSC",
            &[],
            None,
            None,
            &[],
            &PlotStyle::default(),
        );
        assert!(svg.contains("<svg"));
        assert!(svg.contains("FSC"));
        assert!(svg.contains("SSC"));
        assert!(svg.contains("<circle"));
    }

    #[test]
    fn dot_plot_svg_places_the_y_axis_label_clear_of_the_axis() {
        let data = array![[1.0, 2.0], [3.0, 4.0]];
        let svg = dot_plot_svg(
            &data,
            0,
            1,
            "FSC",
            "SSC",
            &[],
            None,
            None,
            &[],
            &PlotStyle::default(),
        );
        let y_label_line = svg
            .lines()
            .find(|l| l.contains(">SSC "))
            .expect("the y axis label must be rendered");
        let after_x = y_label_line.split_once("x=\"").expect("x attribute").1;
        let x = after_x
            .split('"')
            .next()
            .and_then(|s| s.parse::<f64>().ok())
            .expect("y axis label x position");
        // The y axis itself sits at x = 60; the rotated label must be placed just
        // left of it (origin_x - 30 = 30) instead of far away at a fixed offset.
        assert_eq!(x, 30.0, "y axis label x was {x}");
    }

    #[test]
    fn log_axis_dot_plot_renders() {
        let data = array![[1.0, 10.0], [100.0, 1000.0]];
        let style = PlotStyle {
            x_scale: AxisScale::Log,
            y_scale: AxisScale::Log,
            gradient: true,
            ..Default::default()
        };
        let svg = dot_plot_svg(&data, 0, 1, "A", "B", &[], None, None, &[], &style);
        assert!(svg.contains("Log"));
    }

    #[test]
    fn contour_segments_are_produced() {
        // A tight cluster should produce a closed contour.
        let mut pts = Vec::new();
        for _ in 0..1000 {
            pts.push([0.0, 0.0]);
        }
        let grid = density_grid(&pts, -1.0, 1.0, -1.0, 1.0, 32, 32);
        assert!(grid.max() > 0.0);
        let segs = contour_segments(&grid, grid.max() * 0.5);
        assert!(!segs.is_empty());
    }

    #[test]
    fn density_colour_ramp_is_monotonic_channels() {
        let (r0, _, b0) = density_rgb(0.0);
        let (r1, _, b1) = density_rgb(1.0);
        assert!(r1 > r0);
        assert!(b0 > b1);
    }

    #[test]
    fn population_label_respects_field_settings() {
        let info = PopulationLabel {
            name: "CD4+".to_string(),
            count: 25,
            parent_count: Some(100),
            total_count: 200,
            ancestry: vec!["Lymph".to_string(), "CD4+".to_string()],
        };
        let all = GateLabelSettings::default();
        let text = info.text(&all);
        assert!(text.starts_with("CD4+"));
        assert!(text.contains("n=25"));
        assert!(text.contains("25.0% parent"));
        assert!(text.contains("12.5% total"));
        assert!(!text.contains("Lymph / CD4+"), "ancestry is off by default");

        let mut only_name = GateLabelSettings {
            show: true,
            name: true,
            count: false,
            percent_parent: false,
            percent_total: false,
            ancestry: false,
            backgating: false,
            backgate_events: false,
        };
        assert_eq!(info.text(&only_name), "CD4+");
        only_name.ancestry = true;
        assert_eq!(info.text(&only_name), "CD4+  ·  Lymph / CD4+");

        assert_eq!(
            info.text(&GateLabelSettings {
                show: false,
                ..all.clone()
            }),
            ""
        );

        // Zero denominators are omitted rather than producing NaN.
        let empty = PopulationLabel {
            count: 0,
            parent_count: Some(0),
            total_count: 0,
            ..info
        };
        let text = empty.text(&GateLabelSettings {
            ancestry: false,
            ..all
        });
        assert!(!text.contains("NaN"));
        assert!(!text.contains('%'));
    }

    #[test]
    fn backgate_ancestors_walks_root_first_and_stops_at_cycles() {
        let parents = [None, Some(0), Some(1), Some(2), None];
        assert_eq!(backgate_ancestors(&parents, 3), [0, 1, 2]);
        assert_eq!(backgate_ancestors(&parents, 0), Vec::<usize>::new());
        // A cycle must not hang; the repeated index is dropped.
        let cyclic = [Some(1), Some(0), None];
        assert_eq!(backgate_ancestors(&cyclic, 0), [1]);
        // Out-of-range parents are ignored.
        assert!(backgate_ancestors(&[Some(99)], 0).is_empty());
    }

    #[test]
    fn svg_overlay_renders_label_and_dashed_ancestor() {
        let data = array![[1.0, 1.0], [2.0, 2.0], [3.0, 3.0]];
        let overlays = vec![
            PlotOverlay {
                points: vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]],
                stroke: "#ff0000".to_string(),
                fill: None,
                closed: true,
                label: Some(OverlayLabel {
                    text: "CD4+ · 25.0% parent".to_string(),
                    position: [2.0, 2.0],
                    color: "#ff0000".to_string(),
                    size: 11.0,
                }),
                dashed: false,
            },
            PlotOverlay {
                points: vec![[1.0, 1.0], [2.0, 1.0], [2.0, 2.0]],
                stroke: "#00ff00".to_string(),
                fill: None,
                closed: true,
                label: None,
                dashed: true,
            },
        ];
        let svg = dot_plot_svg(
            &data,
            0,
            1,
            "X",
            "Y",
            &[],
            None,
            None,
            &overlays,
            &PlotStyle::default(),
        );
        assert!(svg.contains("CD4+"));
        assert!(svg.contains("<text"));
        assert!(svg.contains("stroke-dasharray"));
    }

    #[test]
    fn histogram_svg_renders() {
        let data = array![[1.0], [2.0], [2.0], [3.0]];
        let svg = histogram_svg(
            &data,
            0,
            "FSC",
            "Count",
            &[],
            None,
            10,
            &PlotStyle::default(),
        );
        assert!(svg.contains("<svg"));
        assert!(svg.contains("FSC"));
        assert!(svg.contains("<rect"));
    }

    #[test]
    fn density_maps_sample_expected_colours() {
        assert_eq!(DensityMap::all().len(), 5);
        assert_eq!(DensityMap::from_id("viridis"), Some(DensityMap::Viridis));
        assert_eq!(
            DensityMap::from_id("greyscale"),
            Some(DensityMap::Greyscale)
        );
        assert_eq!(DensityMap::from_id("zebra"), Some(DensityMap::Zebra));
        assert_eq!(DensityMap::from_id("nonsense"), None);
        assert_eq!(DensityMap::Rainbow.id(), "rainbow");
        assert_eq!(DensityMap::Zebra.label(), "zebra");
        let grey = DensityMap::Greyscale.sample(0.5);
        assert!((grey.0 - 0.5).abs() < 1e-9);
        assert!((grey.2 - 0.5).abs() < 1e-9);
        let zebra = DensityMap::Zebra.sample(0.3);
        assert!((zebra.0 - 0.85).abs() < 1e-9);
        let rainbow = DensityMap::Rainbow.sample(0.0);
        assert!(rainbow.2 > 0.5);
    }

    #[test]
    fn density_legend_renders_svg_gradient() {
        let svg = density_legend_svg(&DensityMap::Viridis, 40.0);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("linearGradient"));
    }

    #[test]
    fn dot_plot_svg_density_renders_map_coloured_points() {
        let data = array![[2.0, 2.0], [2.0, 2.0], [2.0, 2.0]];
        let svg = dot_plot_svg_density(
            &data,
            0,
            1,
            "A",
            "B",
            &[],
            None,
            None,
            &[],
            &PlotStyle::default(),
            &DensityMap::Zebra,
            2,
        );
        assert!(svg.contains("<circle"));
        // Zebra at density 1.0 on a two-band stripe pattern is the light stripe.
        assert!(svg.contains("#d9d9d9"));
    }

    #[test]
    fn data_overlay_dots_render_and_are_labelled() {
        let data = array![[1.0, 1.0], [2.0, 2.0], [3.0, 3.0]];
        let overlay = DataOverlay::dots(
            "Parent",
            "#ff0000",
            vec![[1.0, 3.0], [2.0, 2.0], [3.0, 1.0]],
        );
        let svg = dot_plot_svg_multi(
            &data,
            0,
            1,
            "A",
            "B",
            &[],
            Some((0.0, 4.0)),
            Some((0.0, 4.0)),
            &[],
            &[overlay],
            &PlotStyle::default(),
            None,
        );
        assert!(svg.contains("fill=\"#ff0000\""));
        assert!(svg.contains(">Parent<"));
    }

    #[test]
    fn histogram_overlay_renders_a_step_path() {
        let data = array![[1.0, 1.0], [2.0, 2.0], [3.0, 3.0]];
        let overlay = DataOverlay {
            label: "Hist".to_string(),
            color: "#00aa00".to_string(),
            points: vec![[1.0, 1.0], [1.5, 2.0], [2.0, 3.0], [2.5, 1.0]],
            kind: OverlayKind::Histogram,
            bins: 8,
            density_map: DensityMap::Rainbow,
        };
        let svg = dot_plot_svg_multi(
            &data,
            0,
            1,
            "A",
            "B",
            &[],
            Some((0.0, 4.0)),
            Some((0.0, 4.0)),
            &[],
            &[overlay],
            &PlotStyle::default(),
            None,
        );
        assert!(svg.contains("stroke=\"#00aa00\""));
        assert!(svg.contains("<path d=\"M "));
    }

    #[test]
    fn raw_overlay_renders_as_dots() {
        let data = array![[1.0, 1.0], [2.0, 2.0], [3.0, 3.0]];
        let overlay = DataOverlay {
            label: "Raw".to_string(),
            color: "#3366cc".to_string(),
            points: vec![[1.0, 3.0], [2.0, 2.0], [3.0, 1.0]],
            kind: OverlayKind::Raw,
            bins: 8,
            density_map: DensityMap::Rainbow,
        };
        let svg = dot_plot_svg_multi(
            &data,
            0,
            1,
            "A",
            "B",
            &[],
            Some((0.0, 4.0)),
            Some((0.0, 4.0)),
            &[],
            &[overlay],
            &PlotStyle::default(),
            None,
        );
        assert!(svg.contains("fill=\"#3366cc\""));
        assert!(svg.contains(">Raw<"));
    }

    #[test]
    fn multigraph_panel_assignments_cover_each_mode() {
        let n = 4;
        let by_x = multigraph_panels(n, 1, 2, MultigraphMode::AllByX);
        assert_eq!(by_x.len(), n);
        assert!(by_x.iter().all(|&(x, _)| x == 1));
        let by_y = multigraph_panels(n, 1, 2, MultigraphMode::AllByY);
        assert_eq!(by_y.len(), n);
        assert!(by_y.iter().all(|&(_, y)| y == 2));
        let nxn = multigraph_panels(n, 0, 0, MultigraphMode::NxN);
        assert_eq!(nxn.len(), n * (n - 1));
        assert!(nxn.iter().all(|&(x, y)| x != y));
        assert!(nxn.contains(&(2, 1)));
        assert!(nxn.contains(&(1, 2)));
    }

    #[test]
    fn multigraph_svg_places_every_panel() {
        let data = array![[1.0, 2.0, 3.0, 4.0], [2.0, 3.0, 4.0, 5.0]];
        let labels = vec![
            "P1".to_string(),
            "P2".to_string(),
            "P3".to_string(),
            "P4".to_string(),
        ];
        let panels = multigraph_panels(4, 0, 1, MultigraphMode::AllByX);
        let svg = multigraph_svg(&data, &panels, 2, 240, 180, &labels, &PlotStyle::default());
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>\n"));
        assert_eq!(
            svg.matches("<g transform=\"translate(").count(),
            panels.len()
        );
        // Each panel is labelled with its parameter (and scale suffix).
        assert!(svg.contains("P4"));
        assert!(svg.contains("P1"));
    }

    #[test]
    fn overlay_creation_helpers_build_points_and_layers() {
        let data = array![[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]];
        assert_eq!(
            overlay_points(&data, 0, 1, &[]),
            vec![[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]
        );
        assert_eq!(
            overlay_points(&data, 0, 1, &[2, 0]),
            vec![[5.0, 6.0], [1.0, 2.0]]
        );
        assert!(overlay_points(&data, 5, 1, &[]).is_empty());

        let overlay = data_overlay("Extra", &data, 0, 1, &[0, 1], OverlayKind::Raw, "#ff0000");
        assert_eq!(overlay.label, "Extra");
        assert_eq!(overlay.kind, OverlayKind::Raw);
        assert_eq!(overlay.points.len(), 2);

        let histogram = DataOverlay::histogram("H", "#00ff00", vec![[1.0, 1.0]], 8);
        assert_eq!(histogram.kind, OverlayKind::Histogram);
        assert_eq!(histogram.bins, 8);
        let contour = DataOverlay::contour("C", "#0000ff", vec![[1.0, 1.0]]);
        assert_eq!(contour.kind, OverlayKind::Contour);
        let pseudo = DataOverlay::pseudocolor("P", vec![[1.0, 1.0]], DensityMap::Jet);
        assert_eq!(pseudo.kind, OverlayKind::Pseudocolor);
        assert_eq!(pseudo.density_map, DensityMap::Jet);
        assert_eq!(
            DataOverlay::dots("D", "#111111", vec![])
                .with_kind(OverlayKind::Contour)
                .kind,
            OverlayKind::Contour
        );
    }

    #[test]
    fn plot_presets_apply_display_modes() {
        let base = PlotStyle::default();
        assert_eq!(PlotPreset::all().len(), 5);
        for preset in PlotPreset::all() {
            assert!(!preset.id().is_empty());
            assert!(!preset.label().is_empty());
        }
        let dots = PlotPreset::Dots.apply(&base);
        assert!(!dots.gradient);
        assert!(!dots.contour);
        let contour = PlotPreset::Contour.apply(&base);
        assert!(contour.contour);
        assert!(!contour.gradient);
        let pseudo = PlotPreset::Pseudocolor.apply(&base);
        assert!(pseudo.gradient);
        assert!(!pseudo.contour);
        let zebra = PlotPreset::Zebra.apply(&base);
        assert!(zebra.gradient);
        assert!(zebra.large_dots);
    }

    #[test]
    fn figure_preset_registry_saves_applies_and_replaces() {
        let mut registry = FigurePresetRegistry::with_built_ins();
        assert_eq!(registry.names().len(), 5);
        assert!(registry.get("pseudocolor").unwrap().density_map.is_some());
        assert!(registry.get("contour").unwrap().density_map.is_none());

        let target = PlotStyle {
            width: 800,
            height: 600,
            title: "Figure".into(),
            ..Default::default()
        };
        let applied = registry.apply("pseudocolor", &target).unwrap();
        assert_eq!(applied.width, 800);
        assert_eq!(applied.height, 600);
        assert_eq!(applied.title, "Figure");
        assert!(applied.gradient);

        let custom = FigurePreset::from_plot_preset("custom", PlotPreset::Contour, &target)
            .with_density_map(DensityMap::Zebra)
            .with_mode(MultigraphMode::NxN);
        registry.save(custom);
        assert_eq!(
            registry.get("custom").unwrap().mode,
            Some(MultigraphMode::NxN)
        );
        assert_eq!(
            registry.get("custom").unwrap().density_map,
            Some(DensityMap::Zebra)
        );

        // Replace-by-name keeps the registry length stable.
        let replacement = FigurePreset::new("custom", PlotPreset::Dots.apply(&target));
        registry.save(replacement);
        assert_eq!(registry.names().len(), 6);
        assert!(registry.get("custom").unwrap().mode.is_none());
        assert!(registry.remove("custom"));
        assert!(!registry.remove("custom"));
        assert!(registry.apply("missing", &target).is_none());
    }

    #[test]
    fn multigraph_svg_multi_and_nxn_render_overlays() {
        let data = array![
            [1.0, 2.0, 3.0],
            [2.0, 3.0, 1.0],
            [3.0, 1.0, 2.0],
            [4.0, 5.0, 6.0]
        ];
        let labels = vec!["P1".to_string(), "P2".to_string(), "P3".to_string()];
        let panels = multigraph_panels(3, 0, 0, MultigraphMode::AllByX);
        let overlay = DataOverlay::dots("Extra", "#ff0000", vec![[1.0, 2.0], [2.0, 3.0]]);
        let svg = multigraph_svg_multi(
            &data,
            &panels,
            2,
            200,
            160,
            &labels,
            &PlotStyle::default(),
            &[],
            &[overlay],
            Some(&DensityMap::Viridis),
        );
        assert!(svg.starts_with("<svg"));
        assert_eq!(
            svg.matches("<g transform=\"translate(").count(),
            panels.len()
        );
        assert!(svg.contains(">Extra<"));
        assert!(svg.contains("#ff0000"));

        let nxn = nxn_svg(&data, 3, 200, 160, &labels, &PlotStyle::default());
        assert_eq!(nxn.matches("<g transform=\"translate(").count(), 6);
        let nxn_multi = nxn_svg_multi(
            &data,
            3,
            200,
            160,
            &labels,
            &PlotStyle::default(),
            &[],
            &[],
            None,
        );
        assert_eq!(nxn_multi.matches("<g transform=\"translate(").count(), 6);
    }

    #[test]
    fn tick_notation_formats_auto_scientific_and_decimal() {
        assert_eq!(format_value_with(0.0, TickNotation::Scientific), "0");
        assert_eq!(
            format_value_with(15000.0, TickNotation::Scientific),
            "1.5e4"
        );
        assert_eq!(format_value_with(15000.0, TickNotation::Decimal), "15000");
        assert_eq!(
            format_value_with(0.000123, TickNotation::Decimal),
            "0.000123"
        );
        // Auto uses scientific notation for very large values.
        assert_eq!(format_value_with(1.0e6, TickNotation::Auto), "1e6");
        for n in TickNotation::all() {
            assert_eq!(TickNotation::from_id(n.id()), Some(n));
        }
    }

    #[test]
    fn smoothing_blurs_a_single_spike_and_outlier_clipping_lowers_the_max() {
        let mut grid = density_grid(&[[0.5, 0.5]], 0.0, 1.0, 0.0, 1.0, 9, 9);
        let before = grid.max();
        assert!(before > 0.0);
        smooth_density_grid(&mut grid, 0.5);
        assert!(grid.max() < before);
        // Mass is preserved across the grid.
        let total: f64 = grid.values.iter().sum();
        assert!((total - before).abs() < 1e-9);

        let values = vec![1.0, 1.0, 1.0, 1.0, 100.0];
        assert_eq!(clipped_density_max(&values, 0.0), 100.0);
        assert!(clipped_density_max(&values, 20.0) < 100.0);
        assert_eq!(clipped_density_max(&[], 5.0), 0.0);
    }

    #[test]
    fn median_and_histogram_smoothing_are_stable() {
        assert_eq!(median_of(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median_of(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
        assert_eq!(median_of(&[]), None);
        let smoothed = smooth_counts(&[0, 10, 0], 1.0);
        assert!(smoothed[1] < 10);
        assert!(smoothed[0] > 0);
        assert_eq!(smooth_counts(&[1, 2, 3], 0.0), vec![1, 2, 3]);
    }

    #[test]
    fn dot_plot_honours_v5_visibility_grid_medians_and_large_dots() {
        let data = array![[1.0, 1.0], [2.0, 2.0], [3.0, 3.0], [4.0, 4.0]];
        let style = PlotStyle {
            width: 400,
            height: 300,
            grid: true,
            medians: true,
            large_dots: true,
            show_event_numbers: true,
            outlier_clip_percent: 10.0,
            notation: TickNotation::Scientific,
            ..Default::default()
        };
        let svg = dot_plot_svg(
            &data,
            0,
            1,
            "A",
            "B",
            &[],
            Some((0.0, 5.0)),
            Some((0.0, 5.0)),
            &[],
            &style,
        );
        assert!(svg.contains("#e2e8f0"), "grid lines should be drawn");
        assert!(
            svg.contains("stroke-dasharray=\"5 4\""),
            "median lines should be drawn"
        );
        assert!(svg.contains("n = 4"), "event count should be drawn");
        assert!(
            svg.contains("r=\"4\""),
            "large dots should double the radius"
        );
        assert!(svg.contains("e0"), "scientific ticks should be used");

        // Hidden axis elements are omitted from the export.
        let hidden = PlotStyle {
            show_axis_labels: false,
            show_ticks: false,
            show_numbers: false,
            ..PlotStyle::default()
        };
        let plain = dot_plot_svg(
            &data,
            0,
            1,
            "A",
            "B",
            &[],
            Some((0.0, 5.0)),
            Some((0.0, 5.0)),
            &[],
            &hidden,
        );
        assert!(!plain.contains(">A<"));
        assert!(!plain.contains("text-anchor=\"end\""));
    }

    #[test]
    fn histogram_honours_v5_controls() {
        let data = array![[1.0], [2.0], [2.0], [3.0]];
        let style = PlotStyle {
            width: 400,
            height: 300,
            grid: true,
            medians: true,
            show_event_numbers: true,
            notation: TickNotation::Decimal,
            ..Default::default()
        };
        let svg = histogram_svg(&data, 0, "A", "Count", &[], Some((0.0, 4.0)), 8, &style);
        assert!(svg.contains("#e2e8f0"));
        assert!(svg.contains("stroke-dasharray=\"5 4\""));
        assert!(svg.contains("n = 4"));
        // Decimal notation: no exponent markers in the tick labels.
        assert!(svg.contains(">2.44<"));
        assert!(!svg.contains(">2.44e"));
    }

    #[test]
    fn axis_histogram_counts_bucket_and_clip_values() {
        let counts = axis_histogram_counts(&[0.0, 0.1, 0.9, 1.0, 2.0], 0.0, 1.0, 4);
        assert_eq!(counts.len(), 4);
        assert_eq!(counts.iter().sum::<usize>(), 4);
        assert_eq!(counts[0], 2);
        assert_eq!(counts[3], 2);
        // Flat/empty ranges produce zeroed bins instead of panicking.
        assert_eq!(axis_histogram_counts(&[1.0], 1.0, 1.0, 3), vec![0, 0, 0]);
    }

    #[test]
    fn dot_plot_draws_adjunct_axis_histograms_when_enabled() {
        let data = array![[1.0, 1.0], [2.0, 2.0], [3.0, 3.0], [3.5, 2.5]];
        let plain = dot_plot_svg(
            &data,
            0,
            1,
            "A",
            "B",
            &[],
            Some((0.0, 4.0)),
            Some((0.0, 4.0)),
            &[],
            &PlotStyle::default(),
        );
        assert!(!plain.contains("axis-histograms"));

        let style = PlotStyle {
            axis_histograms: true,
            ..Default::default()
        };
        let svg = dot_plot_svg(
            &data,
            0,
            1,
            "A",
            "B",
            &[],
            Some((0.0, 4.0)),
            Some((0.0, 4.0)),
            &[],
            &style,
        );
        assert!(svg.contains("id=\"axis-histograms\""));
        assert!(svg.contains("fill-opacity=\"0.45\""));
    }

    #[test]
    fn spectral_weight_svg_renders_one_bar_per_source() {
        let svg = spectral_weight_svg(&[1.0, 2.0, 0.0], 30, 10, "#ff0000");
        assert!(svg.starts_with("<svg"));
        assert_eq!(svg.matches("<rect").count(), 3);
    }

    #[test]
    fn table_line_plot_svg_renders_points() {
        let svg = table_line_plot_svg(&[(0.0, 0.0), (1.0, 1.0)], 100, 50);
        assert!(svg.contains("<polyline"));
        assert!(svg.contains("0.0,90.0"));
        assert!(svg.contains("90.0,10.0"));
    }

    #[test]
    fn linear_regression_recovers_perfect_line() {
        let fit = linear_regression(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]).unwrap();
        assert!((fit.slope - 1.0).abs() < 1e-9);
        assert!((fit.intercept).abs() < 1e-9);
        assert!((fit.r_squared - 1.0).abs() < 1e-9);
    }

    #[test]
    fn linear_regression_reports_r_squared_for_noisy_points() {
        let fit = linear_regression(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.1)]).unwrap();
        assert!(fit.slope > 0.0);
        assert!(fit.r_squared > 0.9 && fit.r_squared < 1.0);
    }

    #[test]
    fn linear_regression_needs_two_points() {
        assert!(linear_regression(&[]).is_none());
        assert!(linear_regression(&[(1.0, 2.0)]).is_none());
    }

    #[test]
    fn table_regression_svg_renders_data_and_fit() {
        let svg = table_regression_svg(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)], 100, 50);
        assert!(svg.contains("<polyline"));
        assert!(svg.contains("r^2=1"));
        assert!(svg.contains("10.0,90.0"));
        assert!(svg.contains("90.0,10.0"));
    }

    #[test]
    fn table_box_whisker_svg_renders_box_and_whiskers() {
        let svg = table_box_whisker_svg(&[1.0, 2.0, 3.0, 4.0], 100, 50);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<rect"));
        assert!(svg.contains("<line"));
    }

    #[test]
    fn quartiles_compute_linear_interpolation() {
        assert_eq!(quartiles(&[1.0, 2.0, 3.0, 4.0]), (1.75, 2.5, 3.25));
    }
}
