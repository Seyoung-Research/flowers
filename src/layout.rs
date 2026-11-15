//! Layout editor: compose several rendered plots into one image/export.
//!
//! Each panel is a self-contained SVG document (as produced by
//! [`crate::plot::dot_plot_svg`]). [`render_layout`] arranges them in a grid,
//! and [`render_layout_free`] places them at arbitrary positions and sizes; both
//! inline each panel's contents into a `<g>` with a title. Inlining (rather than
//! nesting `<svg>` documents) keeps the output portable across SVG viewers, and
//! text always carries an explicit font family so raster exports do not fall
//! back to a mismatched font.

use crate::statistics::{DifferenceRow, HeatRange, statistic_label};

/// A single panel in a layout: a title and a pre-rendered SVG document.
#[derive(Debug, Clone, Default)]
pub struct LayoutPanel {
    pub title: String,
    pub svg: String,
}

/// Options controlling how a layout grid is rendered.
#[derive(Debug, Clone)]
pub struct LayoutOptions {
    pub columns: usize,
    pub cell_width: u32,
    pub cell_height: u32,
    /// Height reserved above each panel for its title.
    pub title_height: u32,
    /// Padding around the whole figure.
    pub padding: u32,
    pub background: String,
    pub text_color: String,
    /// Font family used for panel titles (also carried into raster exports so
    /// text does not fall back to a mismatched font).
    pub font_family: String,
    /// Font size (in SVG units) of each panel's title.
    pub title_size: f32,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            columns: 2,
            cell_width: 500,
            cell_height: 450,
            title_height: 28,
            padding: 12,
            background: "#ffffff".to_string(),
            text_color: "#222222".to_string(),
            font_family: "sans-serif".to_string(),
            title_size: 13.0,
        }
    }
}

fn svg_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Returns the markup *inside* an SVG document — everything after the opening
/// `<svg ...>` tag and before the final `</svg>`.
///
/// Panel documents are inlined into a `<g>` rather than nested inside another
/// `<svg>`, which avoids problems some SVG viewers have with nested viewports
/// (a reported cause of "missing plots" in exported layouts).
fn svg_inner(svg: &str) -> &str {
    let start = match svg.find('>') {
        Some(i) => i + 1,
        None => return "",
    };
    let end = svg.rfind("</svg>").unwrap_or(svg.len());
    &svg[start..end.max(start).min(svg.len())]
}

/// Arranges panels in a grid and returns a composite SVG document.
pub fn render_layout(panels: &[LayoutPanel], opts: &LayoutOptions) -> String {
    let cols = opts.columns.max(1);
    let rows = if panels.is_empty() {
        1
    } else {
        panels.len().div_ceil(cols)
    };
    let cell_w = opts.cell_width + opts.padding;
    let cell_h = opts.cell_height + opts.title_height + opts.padding;
    let total_w = cell_w * cols as u32 + opts.padding;
    let total_h = cell_h * rows as u32 + opts.padding;

    let mut svg = String::new();
    svg.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{total_w}" height="{total_h}" viewBox="0 0 {total_w} {total_h}">"#
    ));
    svg.push('\n');
    svg.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{total_w}\" height=\"{total_h}\" fill=\"{}\" />\n",
        opts.background
    ));

    for (i, panel) in panels.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let cx = opts.padding as f64 + col as f64 * cell_w as f64;
        let cy = opts.padding as f64 + row as f64 * cell_h as f64;

        // Title.
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{}\" font-weight=\"bold\" fill=\"{}\" text-anchor=\"middle\">{}</text>\n",
            cx + opts.cell_width as f64 / 2.0,
            cy + 18.0,
            opts.font_family,
            opts.title_size,
            opts.text_color,
            svg_escape(&panel.title)
        ));

        // Inlined panel, offset below the title.
        let inner = svg_inner(&panel.svg);
        svg.push_str(&format!(
            "<g transform=\"translate({:.2},{:.2})\">\n",
            cx,
            cy + opts.title_height as f64
        ));
        svg.push_str(inner);
        if !inner.ends_with('\n') {
            svg.push('\n');
        }
        svg.push_str("</g>\n");
    }

    svg.push_str("</svg>\n");
    svg
}

/// A panel placed at an explicit position and size on a free-form canvas.
///
/// This backs the GUI layout editor, where panels can be dragged and resized
/// like objects in a drawing program before being exported.
#[derive(Debug, Clone)]
pub struct LayoutPlacement {
    pub title: String,
    pub svg: String,
    /// The natural size of `svg` (before scaling to `width`/`height`).
    pub svg_width: u32,
    pub svg_height: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Font size (in SVG units) of the panel's title.
    pub title_size: f32,
}

impl Default for LayoutPlacement {
    fn default() -> Self {
        Self {
            title: String::new(),
            svg: String::new(),
            svg_width: 0,
            svg_height: 0,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            title_size: 13.0,
        }
    }
}

/// Composes freely-placed panels into a single SVG canvas.
///
/// Each panel's SVG is inlined and scaled to fit its `width`/`height`, with its
/// title drawn above it.
pub fn render_layout_free(
    canvas_width: u32,
    canvas_height: u32,
    placements: &[LayoutPlacement],
    opts: &LayoutOptions,
) -> String {
    let canvas_width = canvas_width.max(1);
    let canvas_height = canvas_height.max(1);
    let mut svg = String::new();
    svg.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{canvas_width}" height="{canvas_height}" viewBox="0 0 {canvas_width} {canvas_height}">"#
    ));
    svg.push('\n');
    svg.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{canvas_width}\" height=\"{canvas_height}\" fill=\"{}\" />\n",
        opts.background
    ));

    for p in placements {
        // Title above the panel.
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{}\" font-weight=\"bold\" fill=\"{}\" text-anchor=\"middle\">{}</text>\n",
            p.x as f64 + p.width as f64 / 2.0,
            p.y as f64 + 12.0,
            opts.font_family,
            p.title_size,
            opts.text_color,
            svg_escape(&p.title)
        ));

        let nat_w = p.svg_width.max(1) as f32;
        let nat_h = p.svg_height.max(1) as f32;
        let sx = p.width / nat_w;
        let sy = p.height / nat_h;
        svg.push_str(&format!(
            "<g transform=\"translate({:.2},{:.2}) scale({:.4},{:.4})\">\n",
            p.x,
            p.y + opts.title_height as f32,
            sx,
            sy
        ));
        let inner = svg_inner(&p.svg);
        svg.push_str(inner);
        if !inner.ends_with('\n') {
            svg.push('\n');
        }
        svg.push_str("</g>\n");
    }

    svg.push_str("</svg>\n");
    svg
}

/// Alignment of text inside a layout object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
}

/// A styled text layout object with live FJML field interpolation (L1).
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutText {
    pub content: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub color: String,
    pub opacity: f32,
    pub rotation_deg: f32,
    pub align: TextAlign,
}

/// Shape kinds for layout objects (L1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShapeKind {
    #[default]
    Rect,
    Ellipse,
    Line,
    Arrow,
    WebBox,
}

/// A primitive shape layout object with style controls (L1): fill/tint/gradient,
/// line style/width, opacity, shadow and rotation.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutShape {
    pub kind: ShapeKind,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub fill: Option<String>,
    pub stroke: Option<String>,
    pub stroke_width: f32,
    pub opacity: f32,
    pub shadow: bool,
    pub rotation_deg: f32,
}

/// A grouped set of layout objects (L1).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayoutGroup {
    pub name: String,
    pub objects: Vec<LayoutObject>,
}

/// A legend layout object: a title plus labeled color swatches, used for heat
/// maps and gating legends (L4).
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutLegend {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub title: String,
    pub entries: Vec<LayoutLegendEntry>,
}

/// One labeled color swatch in a [`LayoutLegend`] (L4).
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutLegendEntry {
    pub label: String,
    pub color: String,
}

impl LayoutLegend {
    pub fn new(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        title: &str,
        entries: Vec<LayoutLegendEntry>,
    ) -> Self {
        Self {
            x,
            y,
            width,
            height,
            title: title.to_string(),
            entries,
        }
    }
}

/// A heat-map layout object: a matrix of values rendered as colored cells, with
/// row/column labels and a title (L4 plate displays).
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutHeatMap {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub title: String,
    pub row_labels: Vec<String>,
    pub col_labels: Vec<String>,
    pub values: Vec<Vec<f64>>,
    pub heat: Option<HeatRange>,
}

/// A layout object: styled text, a shape, a group, a table or a heat map (L1/L4).
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutObject {
    Text(LayoutText),
    Shape(LayoutShape),
    Group(LayoutGroup),
    Table(LayoutTable),
    Legend(LayoutLegend),
    HeatMap(LayoutHeatMap),
}

/// A table layout object: a titled grid of columns and rows with optional
/// heat-map flag highlighting, rendered into the SVG canvas (L4).
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutTable {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub heat: Option<HeatRange>,
}

impl LayoutTable {
    /// Create a table layout object with a title, header columns and data rows.
    pub fn new(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        title: &str,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    ) -> Self {
        Self {
            x,
            y,
            width,
            height,
            title: title.to_string(),
            columns,
            rows,
            heat: None,
        }
    }

    /// Enable heat-map flag highlighting (difference tables flag large
    /// deviations; statistic tables flag out-of-range values).
    pub fn with_heat(mut self, heat: HeatRange) -> Self {
        self.heat = Some(heat);
        self
    }

    /// True when this row is heat-map flagged: a numeric cell whose absolute
    /// value exceeds the heat maximum.
    pub fn is_flagged(&self, row: &[String]) -> bool {
        let Some(heat) = &self.heat else {
            return false;
        };
        row.iter().any(|c| {
            c.trim()
                .parse::<f64>()
                .map(|v| v.abs() > heat.max)
                .unwrap_or(false)
        })
    }
}

impl LayoutObject {
    /// Build a difference-table layout object from test-vs-control difference
    /// rows with heat-map flagging (L4).
    pub fn difference_table(title: &str, rows: &[DifferenceRow], heat: Option<HeatRange>) -> Self {
        let mut table = LayoutTable {
            x: 0.0,
            y: 0.0,
            width: 600.0,
            height: 24.0 + (1.0 + rows.len() as f32) * 18.0,
            title: title.to_string(),
            columns: vec![
                "parameter".to_string(),
                "statistic".to_string(),
                "test".to_string(),
                "control".to_string(),
                "difference".to_string(),
            ],
            rows: rows
                .iter()
                .map(|r| {
                    vec![
                        r.parameter.clone(),
                        statistic_label(&r.statistic),
                        r.test_value.to_string(),
                        r.control_value.to_string(),
                        r.difference.to_string(),
                    ]
                })
                .collect(),
            heat: None,
        };
        if let Some(h) = heat {
            table = table.with_heat(h);
        }
        Self::Table(table)
    }
}

/// Context for live FJML field resolution in layout text (L1).
#[derive(Debug, Clone, Default)]
pub struct FjmlContext<'a> {
    pub sample_name: &'a str,
    pub statistic: Option<&'a str>,
    pub keyword: Option<&'a str>,
    pub date: &'a str,
}

impl<'a> FjmlContext<'a> {
    pub fn new(
        sample_name: &'a str,
        statistic: Option<&'a str>,
        keyword: Option<&'a str>,
        date: &'a str,
    ) -> Self {
        Self {
            sample_name,
            statistic,
            keyword,
            date,
        }
    }
}

/// Resolve `{{...}}` FJML tokens against a context (L1): sample names, statistic
/// labels, keyword values and dates.
pub fn resolve_fjml(text: &str, ctx: &FjmlContext) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = start + 2;
        let end = match rest[after..].find("}}") {
            Some(i) => after + i,
            None => {
                out.push_str(&rest[start..]);
                break;
            }
        };
        let token = &rest[after..end];
        let resolved = if token.strip_prefix("sample.").is_some() {
            ctx.sample_name.to_string()
        } else if token.strip_prefix("stat:").is_some() {
            ctx.statistic.unwrap_or("").to_string()
        } else if token.strip_prefix("keyword:").is_some() {
            ctx.keyword.unwrap_or("").to_string()
        } else if token == "date" {
            ctx.date.to_string()
        } else {
            token.to_string()
        };
        out.push_str(&resolved);
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Heat-map color for a value, lerping from blue (low) to red (high) across the
/// heat range (L4 plate heat maps).
fn heat_color(heat: &HeatRange, value: f64) -> String {
    let span = heat.max - heat.min;
    let pos = if span > 0.0 {
        ((value - heat.min) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let r = lerp(0x3b as f64, 0xef as f64, pos).round() as u8;
    let g = lerp(0x82 as f64, 0x44 as f64, pos).round() as u8;
    let b = lerp(0xf6 as f64, 0x2b as f64, pos).round() as u8;
    let mut hex = String::from("#");
    hex.push_str(&format!("{:02x}", r));
    hex.push_str(&format!("{:02x}", g));
    hex.push_str(&format!("{:02x}", b));
    hex
}

/// Render a table layout object into an SVG fragment (L4): titled grid with
/// header row, data rows and optional heat-map flag highlighting.
fn render_layout_table(t: &LayoutTable, opts: &LayoutOptions) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\" stroke=\"none\"/>",
        t.x, t.y, t.width, t.height, opts.background
    ));
    s.push_str(&format!(
        "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{}\" font-weight=\"bold\" fill=\"{}\" text-anchor=\"middle\">{}</text>",
        t.x + t.width / 2.0,
        t.y + 14.0,
        opts.font_family,
        opts.title_size,
        opts.text_color,
        svg_escape(&t.title)
    ));
    let title_h = 18.0f32;
    let body_h = (t.height - title_h).max(0.0);
    let drawn = 1 + t.rows.len();
    let row_h = if drawn > 0 {
        body_h / drawn as f32
    } else {
        0.0
    };
    let col_w = if !t.columns.is_empty() {
        t.width / t.columns.len() as f32
    } else {
        0.0
    };
    let font = 10.0f32;
    if !t.columns.is_empty() {
        let row_top = t.y + title_h;
        for (c, label) in t.columns.iter().enumerate() {
            s.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{}\" font-weight=\"bold\" fill=\"{}\">{}</text>",
                t.x + c as f32 * col_w + 4.0,
                row_top + 4.0 + font,
                opts.font_family,
                font,
                opts.text_color,
                svg_escape(label)
            ));
        }
    }
    for (i, row) in t.rows.iter().enumerate() {
        let row_top = t.y + title_h + (1 + i) as f32 * row_h;
        if let Some(h) = t.heat.as_ref()
            && t.is_flagged(row)
        {
            let value = row
                .iter()
                .filter_map(|c| c.trim().parse::<f64>().ok())
                .map(|v| v.abs())
                .fold(0.0f64, f64::max);
            s.push_str(&format!(
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\" fill-opacity=\"0.18\"/>",
                    t.x,
                    row_top,
                    col_w,
                    row_h,
                    heat_color(h, value)
                ));
        }
        for (c, cell) in row.iter().enumerate() {
            s.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\">{}</text>",
                t.x + c as f32 * col_w + 4.0,
                row_top + 4.0 + font,
                opts.font_family,
                font,
                opts.text_color,
                svg_escape(cell)
            ));
        }
    }
    s
}

/// Render a legend layout object into an SVG fragment (L4): title plus swatches.
fn render_layout_legend(l: &LayoutLegend, opts: &LayoutOptions) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{}\" font-weight=\"bold\" fill=\"{}\" text-anchor=\"middle\">{}</text>",
        l.x + l.width / 2.0,
        l.y + 14.0,
        opts.font_family,
        opts.title_size,
        opts.text_color,
        svg_escape(&l.title)
    ));
    let mut y = l.y + 22.0;
    for e in &l.entries {
        s.push_str(&format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"12\" height=\"12\" fill=\"{}\" stroke=\"none\"/>",
            l.x,
            y,
            e.color
        ));
        s.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"10\" fill=\"{}\">{}</text>",
            l.x + 16.0,
            y + 10.0,
            opts.font_family,
            opts.text_color,
            svg_escape(&e.label)
        ));
        y += 18.0;
    }
    s
}

/// Render a heat-map layout object into an SVG fragment (L4): titled grid of
/// colored cells with row/column labels.
fn render_layout_heat_map(m: &LayoutHeatMap, opts: &LayoutOptions) -> String {
    let heat = m.heat.clone().unwrap_or_default();
    let mut s = String::new();
    s.push_str(&format!(
        "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{}\" font-weight=\"bold\" fill=\"{}\" text-anchor=\"middle\">{}</text>",
        m.x + m.width / 2.0,
        m.y + 14.0,
        opts.font_family,
        opts.title_size,
        opts.text_color,
        svg_escape(&m.title)
    ));
    let title_h = 18.0f32;
    let map_y = m.y + title_h;
    let rows = m.values.len().max(1);
    let cols = m.col_labels.len().max(1);
    let cell_w = m.width / cols as f32;
    let cell_h = (m.height - title_h) / rows as f32;
    for (r, row) in m.values.iter().enumerate() {
        for (c, v) in row.iter().enumerate().take(cols) {
            s.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\" stroke=\"none\"/>",
                m.x + c as f32 * cell_w,
                map_y + r as f32 * cell_h,
                cell_w,
                cell_h,
                heat_color(&heat, *v)
            ));
        }
        s.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"10\" fill=\"{}\">{}</text>",
            m.x,
            map_y + r as f32 * cell_h + cell_h * 0.3,
            opts.font_family,
            opts.text_color,
            svg_escape(&m.row_labels[r])
        ));
    }
    for (c, label) in m.col_labels.iter().enumerate() {
        s.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"10\" fill=\"{}\">{}</text>",
            m.x + c as f32 * cell_w,
            m.y + 12.0,
            opts.font_family,
            opts.text_color,
            svg_escape(label)
        ));
    }
    s
}

/// Render a layout object (text / shape / group / table / legend / heat map) into
/// an SVG document fragment (L1/L4).
pub fn layout_object_svg(obj: &LayoutObject, ctx: &FjmlContext, opts: &LayoutOptions) -> String {
    match obj {
        LayoutObject::Text(t) => {
            let content = resolve_fjml(&t.content, ctx);
            let anchor_x = if t.align == TextAlign::Center {
                t.x + t.width / 2.0
            } else {
                t.x
            };
            let mut s = format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\" opacity=\"{}\" text-anchor=\"middle\">{}</text>",
                anchor_x,
                t.y,
                opts.font_family,
                t.font_size,
                t.color,
                t.opacity,
                svg_escape(&content)
            );
            if t.rotation_deg.abs() > 1e-6 {
                s = format!(
                    "<g transform=\"rotate({:.2} {:.2} {:.2})\">{}</g>",
                    t.rotation_deg,
                    t.x + t.width / 2.0,
                    t.y + t.height / 2.0,
                    s
                );
            }
            s
        }
        LayoutObject::Shape(sh) => {
            let opacity = sh.opacity;
            let fill = sh.fill.clone().unwrap_or_else(|| opts.background.clone());
            let stroke = sh.stroke.clone().unwrap_or_else(|| "#222222".to_string());
            let drawn = match sh.kind {
                ShapeKind::Rect => format!(
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" opacity=\"{}\"/>",
                    sh.x, sh.y, sh.width, sh.height, fill, stroke, sh.stroke_width, opacity
                ),
                ShapeKind::Ellipse => format!(
                    "<ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" opacity=\"{}\"/>",
                    sh.x + sh.width / 2.0,
                    sh.y + sh.height / 2.0,
                    sh.width / 2.0,
                    sh.height / 2.0,
                    fill,
                    stroke,
                    sh.stroke_width,
                    opacity
                ),
                ShapeKind::Line | ShapeKind::Arrow => {
                    let (x2, y2) = if sh.kind == ShapeKind::Arrow {
                        (sh.x + sh.width, sh.y + sh.height)
                    } else {
                        (sh.x, sh.y)
                    };
                    format!(
                        "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"{}\" opacity=\"{}\"/>",
                        sh.x, sh.y, x2, y2, stroke, sh.stroke_width, opacity
                    )
                }
                ShapeKind::WebBox => format!(
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-dasharray=\"4 4\" opacity=\"{}\"/>",
                    sh.x, sh.y, sh.width, sh.height, stroke, sh.stroke_width, opacity
                ),
            };
            if sh.shadow {
                format!(
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"none\" stroke=\"#000000\" stroke-width=\"1\" opacity=\"0.25\"/>",
                    sh.x, sh.y, sh.width, sh.height
                )
            } else {
                drawn
            }
        }
        LayoutObject::Group(g) => {
            let mut s = String::new();
            if !g.name.is_empty() {
                s.push_str(&format!(
                    "<text x=\"0\" y=\"0\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\">{}</text>",
                    opts.font_family,
                    12.0,
                    opts.text_color,
                    svg_escape(&g.name)
                ));
            }
            for child in &g.objects {
                s.push_str(&layout_object_svg(child, ctx, opts));
            }
            s
        }
        LayoutObject::Table(t) => render_layout_table(t, opts),
        LayoutObject::Legend(l) => render_layout_legend(l, opts),
        LayoutObject::HeatMap(m) => render_layout_heat_map(m, opts),
    }
}

/// Serialize a set of layout objects into a single SVG document (L2): used for
/// clipboard export / copy of layout objects, with or without a background
/// (transparency) rect.
pub fn layout_objects_to_svg(
    objects: &[LayoutObject],
    ctx: &FjmlContext,
    opts: &LayoutOptions,
    width: u32,
    height: u32,
    transparent: bool,
) -> String {
    let mut inner = String::new();
    for o in objects {
        inner.push_str(&layout_object_svg(o, ctx, opts));
        inner.push('\n');
    }
    let background = if transparent {
        String::new()
    } else {
        format!(
            "<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"{}\" />",
            opts.background
        )
    };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">{background}{inner}</svg>"
    )
}

/// Copy layout objects to the system clipboard as SVG text (L2 clipboard
/// export; `with_transparency` omits the background rect).
pub fn copy_layout_objects_to_clipboard(
    objects: &[LayoutObject],
    with_transparency: bool,
) -> Result<(), String> {
    let svg = layout_objects_to_svg(
        objects,
        &FjmlContext::new("", None, None, ""),
        &LayoutOptions::default(),
        800,
        600,
        with_transparency,
    );
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("clipboard unavailable: {e}"))?;
    clipboard
        .set_text(&svg)
        .map_err(|e| format!("clipboard write failed: {e}"))?;
    Ok(())
}

/// Read layout objects back from the system clipboard as SVG text (L2
/// clipboard paste).
pub fn paste_layout_objects_from_clipboard() -> Result<String, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("clipboard unavailable: {e}"))?;
    clipboard
        .get_text()
        .map_err(|e| format!("clipboard read failed: {e}"))
}

/// A rectangle of merged cells in a grid layout (L3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridMerge {
    pub row: usize,
    pub column: usize,
    pub row_span: usize,
    pub column_span: usize,
}

impl GridMerge {
    /// True when this merged rectangle covers `(row, column)`.
    pub fn covers(&self, row: usize, column: usize) -> bool {
        row >= self.row
            && row < self.row + self.row_span
            && column >= self.column
            && column < self.column + self.column_span
    }
}

/// A grid layout: rows x columns of layout cells with style controls (L3).
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutGrid {
    pub columns: usize,
    pub rows: usize,
    /// Default column width; `column_widths` holds per-column overrides.
    pub column_width: f32,
    /// Default row height; `row_heights` holds per-row overrides.
    pub row_height: f32,
    pub column_widths: Vec<f32>,
    pub row_heights: Vec<f32>,
    pub merges: Vec<GridMerge>,
    pub padding: f32,
    pub border: bool,
    pub axis_labels: bool,
}

impl Default for LayoutGrid {
    fn default() -> Self {
        Self {
            columns: 2,
            rows: 2,
            column_width: 500.0,
            row_height: 450.0,
            column_widths: Vec::new(),
            row_heights: Vec::new(),
            merges: Vec::new(),
            padding: 12.0,
            border: true,
            axis_labels: true,
        }
    }
}

impl LayoutGrid {
    /// Effective width of every column, falling back to `column_width`.
    pub fn column_sizes(&self) -> Vec<f32> {
        if self.column_widths.len() == self.columns {
            self.column_widths.clone()
        } else {
            vec![self.column_width; self.columns]
        }
    }

    /// Effective height of every row, falling back to `row_height`.
    pub fn row_sizes(&self) -> Vec<f32> {
        if self.row_heights.len() == self.rows {
            self.row_heights.clone()
        } else {
            vec![self.row_height; self.rows]
        }
    }

    /// True when the cell is covered by a merged region.
    pub fn is_merged(&self, row: usize, column: usize) -> bool {
        self.merges.iter().any(|merge| merge.covers(row, column))
    }

    /// Top-left corner of a cell relative to the grid body (inside padding).
    pub fn cell_origin(&self, row: usize, column: usize) -> (f32, f32) {
        let widths = self.column_sizes();
        let heights = self.row_sizes();
        let column = column.min(widths.len());
        let row = row.min(heights.len());
        let x =
            self.padding + widths.iter().take(column).sum::<f32>() + self.padding * column as f32;
        let y = self.padding + heights.iter().take(row).sum::<f32>() + self.padding * row as f32;
        (x, y)
    }
}

/// Clamp merged regions to the current grid and drop empty ones (L3).
fn grid_normalize_merges(grid: &mut LayoutGrid) {
    let columns = grid.columns;
    let rows = grid.rows;
    grid.merges.retain_mut(|merge| {
        if merge.row >= rows || merge.column >= columns {
            return false;
        }
        merge.row_span = merge.row_span.max(1).min(rows - merge.row);
        merge.column_span = merge.column_span.max(1).min(columns - merge.column);
        merge.row_span > 1 || merge.column_span > 1
    });
}

fn grid_merges_overlap(left: &GridMerge, right: &GridMerge) -> bool {
    left.row < right.row + right.row_span
        && right.row < left.row + left.row_span
        && left.column < right.column + right.column_span
        && right.column < left.column + left.column_span
}

/// Insert a row at `index`, shifting merged regions that span it (L3).
pub fn grid_insert_row(grid: &mut LayoutGrid, index: usize) {
    let index = index.min(grid.rows);
    let mut heights = grid.row_sizes();
    heights.insert(index, grid.row_height);
    grid.rows = grid.rows.saturating_add(1);
    grid.row_heights = heights;
    for merge in &mut grid.merges {
        if index <= merge.row {
            merge.row += 1;
        } else if index < merge.row + merge.row_span {
            merge.row_span += 1;
        }
    }
    grid_normalize_merges(grid);
}

/// Delete the row at `index`, shrinking or removing merged regions (L3).
pub fn grid_delete_row(grid: &mut LayoutGrid, index: usize) {
    if index >= grid.rows {
        return;
    }
    let mut heights = grid.row_sizes();
    heights.remove(index);
    grid.rows -= 1;
    grid.row_heights = heights;
    for merge in &mut grid.merges {
        if index < merge.row {
            merge.row -= 1;
        } else if index < merge.row + merge.row_span {
            merge.row_span = merge.row_span.saturating_sub(1);
        }
    }
    grid_normalize_merges(grid);
}

/// Insert a column at `index`, shifting merged regions that span it (L3).
pub fn grid_insert_column(grid: &mut LayoutGrid, index: usize) {
    let index = index.min(grid.columns);
    let mut widths = grid.column_sizes();
    widths.insert(index, grid.column_width);
    grid.columns = grid.columns.saturating_add(1);
    grid.column_widths = widths;
    for merge in &mut grid.merges {
        if index <= merge.column {
            merge.column += 1;
        } else if index < merge.column + merge.column_span {
            merge.column_span += 1;
        }
    }
    grid_normalize_merges(grid);
}

/// Delete the column at `index`, shrinking or removing merged regions (L3).
pub fn grid_delete_column(grid: &mut LayoutGrid, index: usize) {
    if index >= grid.columns {
        return;
    }
    let mut widths = grid.column_sizes();
    widths.remove(index);
    grid.columns -= 1;
    grid.column_widths = widths;
    for merge in &mut grid.merges {
        if index < merge.column {
            merge.column -= 1;
        } else if index < merge.column + merge.column_span {
            merge.column_span = merge.column_span.saturating_sub(1);
        }
    }
    grid_normalize_merges(grid);
}

/// Merge a rectangle of grid cells (L3).
///
/// Returns `false` when the rectangle lies outside the grid, covers a single
/// cell, or overlaps an existing merged region.
pub fn grid_merge_cells(
    grid: &mut LayoutGrid,
    row: usize,
    column: usize,
    row_span: usize,
    column_span: usize,
) -> bool {
    let row_span = row_span.max(1);
    let column_span = column_span.max(1);
    if row >= grid.rows || column >= grid.columns {
        return false;
    }
    if row_span > grid.rows - row || column_span > grid.columns - column {
        return false;
    }
    if row_span == 1 && column_span == 1 {
        return false;
    }
    let candidate = GridMerge {
        row,
        column,
        row_span,
        column_span,
    };
    if grid
        .merges
        .iter()
        .any(|existing| grid_merges_overlap(existing, &candidate))
    {
        return false;
    }
    grid.merges.push(candidate);
    grid.merges.sort_by_key(|merge| (merge.row, merge.column));
    true
}

/// Split (unmerge) the merged region containing `(row, column)` (L3).
pub fn grid_split_cell(grid: &mut LayoutGrid, row: usize, column: usize) -> bool {
    let before = grid.merges.len();
    grid.merges.retain(|merge| !merge.covers(row, column));
    grid.merges.len() != before
}

/// Scale every cell uniformly so the grid fits within the given page size
/// (L3 autofit). Returns the applied scale factor.
pub fn grid_autofit(grid: &mut LayoutGrid, page_width: f32, page_height: f32) -> f32 {
    let columns = grid.columns.max(1);
    let rows = grid.rows.max(1);
    let mut widths = grid.column_sizes();
    let mut heights = grid.row_sizes();
    if widths.is_empty() {
        widths = vec![grid.column_width];
    }
    if heights.is_empty() {
        heights = vec![grid.row_height];
    }
    let fixed_w = grid.padding * (columns + 1) as f32;
    let fixed_h = grid.padding * (rows + 1) as f32;
    let available_w = page_width - fixed_w;
    let available_h = page_height - fixed_h;
    let content_w = widths.iter().sum::<f32>();
    let content_h = heights.iter().sum::<f32>();
    if available_w <= 0.0 || available_h <= 0.0 || content_w <= 0.0 || content_h <= 0.0 {
        return 1.0;
    }
    let scale = (available_w / content_w).min(available_h / content_h);
    if (scale - 1.0).abs() <= f32::EPSILON {
        return 1.0;
    }
    for width in &mut widths {
        *width *= scale;
    }
    for height in &mut heights {
        *height *= scale;
    }
    grid.column_widths = widths;
    grid.row_heights = heights;
    grid.column_width *= scale;
    grid.row_height *= scale;
    scale
}

/// Give all columns and rows the same size, equalizing cell spacing (L3).
pub fn grid_equal_spacing(grid: &mut LayoutGrid) {
    if let Some(max) = grid.column_sizes().into_iter().reduce(f32::max) {
        grid.column_widths = vec![max; grid.columns];
        grid.column_width = max;
    }
    if let Some(max) = grid.row_sizes().into_iter().reduce(f32::max) {
        grid.row_heights = vec![max; grid.rows];
        grid.row_height = max;
    }
}

/// Spreadsheet-style column label: A, B, ..., Z, AA, AB, ... (L3).
fn grid_column_label(mut index: usize) -> String {
    let mut label = String::new();
    loop {
        label.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    label
}

/// Render a grid layout as an SVG document with cell borders, padding, merged
/// cells and axis labels on the grid edges (L3).
pub fn render_grid_layout(grid: &LayoutGrid, opts: &LayoutOptions) -> String {
    let columns = grid.columns.max(1);
    let rows = grid.rows.max(1);
    let mut widths = grid.column_sizes();
    let mut heights = grid.row_sizes();
    if widths.len() != columns {
        widths = vec![grid.column_width; columns];
    }
    if heights.len() != rows {
        heights = vec![grid.row_height; rows];
    }
    let (left_margin, top_margin, bottom_margin) = if grid.axis_labels {
        (30.0, 20.0, 18.0)
    } else {
        (0.0, 0.0, 0.0)
    };
    let total_w =
        left_margin + grid.padding + widths.iter().sum::<f32>() + grid.padding * columns as f32;
    let total_h = top_margin
        + grid.padding
        + heights.iter().sum::<f32>()
        + grid.padding * rows as f32
        + bottom_margin;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">",
        width = total_w as u32,
        height = total_h as u32
    );
    if grid.border {
        svg.push_str(&format!(
            "<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"{}\" stroke=\"#999999\" stroke-width=\"1\"/>",
            opts.background, width = total_w as u32, height = total_h as u32
        ));
    }
    let column_x = |column: usize| {
        left_margin
            + grid.padding
            + widths[..column].iter().sum::<f32>()
            + grid.padding * column as f32
    };
    let row_y = |row: usize| {
        top_margin + grid.padding + heights[..row].iter().sum::<f32>() + grid.padding * row as f32
    };
    for r in 0..rows {
        for c in 0..columns {
            let origin = grid
                .merges
                .iter()
                .find(|merge| merge.row == r && merge.column == c);
            if origin.is_none() && grid.is_merged(r, c) {
                continue;
            }
            let (row_span, column_span) = origin
                .map(|merge| {
                    (
                        merge.row_span.max(1).min(rows - r),
                        merge.column_span.max(1).min(columns - c),
                    )
                })
                .unwrap_or((1, 1));
            let x = column_x(c);
            let y = row_y(r);
            let w = widths[c..c + column_span].iter().sum::<f32>()
                + grid.padding * (column_span - 1) as f32;
            let h =
                heights[r..r + row_span].iter().sum::<f32>() + grid.padding * (row_span - 1) as f32;
            svg.push_str(&format!(
                "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" fill=\"none\" stroke=\"#999999\" stroke-width=\"1\"/>"
            ));
        }
    }
    if grid.axis_labels {
        for (c, width) in widths.iter().enumerate().take(columns) {
            let x = column_x(c) + width / 2.0;
            svg.push_str(&format!(
                "<text x=\"{x:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\">{}</text>",
                top_margin / 2.0 + 4.0,
                opts.font_family,
                opts.title_size,
                opts.text_color,
                grid_column_label(c)
            ));
        }
        for (r, height) in heights.iter().enumerate().take(rows) {
            let y = row_y(r) + height / 2.0;
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{y:.1}\" text-anchor=\"end\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\">{}</text>",
                left_margin - 6.0,
                opts.font_family,
                opts.title_size,
                opts.text_color,
                r + 1
            ));
        }
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\">columns={columns}</text>",
            left_margin + grid.padding,
            top_margin - 6.0,
            opts.font_family,
            opts.title_size,
            opts.text_color
        ));
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\">rows={rows}</text>",
            left_margin,
            total_h - 4.0,
            opts.font_family,
            opts.title_size,
            opts.text_color
        ));
    }
    svg.push_str("</svg>");
    svg
}

/// Alignment mode for selected layout objects (L6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignMode {
    Left,
    CenterX,
    Right,
    Top,
    CenterY,
    Bottom,
}

/// Axis used when distributing layout objects (L6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributeAxis {
    Horizontal,
    Vertical,
}

/// Bounds of a layout object in layout coordinates (L6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl ObjectBounds {
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn center_x(&self) -> f32 {
        self.x + self.width / 2.0
    }

    pub fn center_y(&self) -> f32 {
        self.y + self.height / 2.0
    }
}

impl LayoutObject {
    /// Bounding box of this object. Groups return the union of their children;
    /// an empty group has no bounds (L6).
    pub fn bounds(&self) -> Option<ObjectBounds> {
        match self {
            LayoutObject::Text(text) => Some(ObjectBounds {
                x: text.x,
                y: text.y,
                width: text.width,
                height: text.height,
            }),
            LayoutObject::Shape(shape) => Some(ObjectBounds {
                x: shape.x,
                y: shape.y,
                width: shape.width,
                height: shape.height,
            }),
            LayoutObject::Group(group) => {
                let mut bounds: Option<ObjectBounds> = None;
                for child in &group.objects {
                    if let Some(child_bounds) = child.bounds() {
                        bounds = Some(match bounds {
                            Some(current) => {
                                let x = current.x.min(child_bounds.x);
                                let y = current.y.min(child_bounds.y);
                                let right = current.right().max(child_bounds.right());
                                let bottom = current.bottom().max(child_bounds.bottom());
                                ObjectBounds {
                                    x,
                                    y,
                                    width: right - x,
                                    height: bottom - y,
                                }
                            }
                            None => child_bounds,
                        });
                    }
                }
                bounds
            }
            LayoutObject::Table(table) => Some(ObjectBounds {
                x: table.x,
                y: table.y,
                width: table.width,
                height: table.height,
            }),
            LayoutObject::Legend(legend) => Some(ObjectBounds {
                x: legend.x,
                y: legend.y,
                width: legend.width,
                height: legend.height,
            }),
            LayoutObject::HeatMap(heat_map) => Some(ObjectBounds {
                x: heat_map.x,
                y: heat_map.y,
                width: heat_map.width,
                height: heat_map.height,
            }),
        }
    }

    /// Translate this object (and every child of a group) by `(dx, dy)` (L6).
    pub fn translate(&mut self, dx: f32, dy: f32) {
        match self {
            LayoutObject::Text(text) => {
                text.x += dx;
                text.y += dy;
            }
            LayoutObject::Shape(shape) => {
                shape.x += dx;
                shape.y += dy;
            }
            LayoutObject::Group(group) => {
                for child in &mut group.objects {
                    child.translate(dx, dy);
                }
            }
            LayoutObject::Table(table) => {
                table.x += dx;
                table.y += dy;
            }
            LayoutObject::Legend(legend) => {
                legend.x += dx;
                legend.y += dy;
            }
            LayoutObject::HeatMap(heat_map) => {
                heat_map.x += dx;
                heat_map.y += dy;
            }
        }
    }
}

/// Align the selected objects to the bounding box of the selection (L6).
pub fn align_objects(objects: &mut [LayoutObject], selection: &[usize], mode: AlignMode) {
    align_objects_with_reference(objects, selection, selection, mode);
}

/// Align `movable` objects to the bounding box of `reference` objects (L6).
fn align_objects_with_reference(
    objects: &mut [LayoutObject],
    reference: &[usize],
    movable: &[usize],
    mode: AlignMode,
) {
    let bounds: Vec<ObjectBounds> = reference
        .iter()
        .copied()
        .filter_map(|index| objects.get(index).and_then(|object| object.bounds()))
        .collect();
    if bounds.is_empty() {
        return;
    }
    let target_left = bounds.iter().map(|b| b.x).fold(f32::INFINITY, f32::min);
    let target_right = bounds
        .iter()
        .map(|b| b.right())
        .fold(f32::NEG_INFINITY, f32::max);
    let target_top = bounds.iter().map(|b| b.y).fold(f32::INFINITY, f32::min);
    let target_bottom = bounds
        .iter()
        .map(|b| b.bottom())
        .fold(f32::NEG_INFINITY, f32::max);
    let target_center_x = (target_left + target_right) / 2.0;
    let target_center_y = (target_top + target_bottom) / 2.0;
    for index in movable {
        let Some(object) = objects.get_mut(*index) else {
            continue;
        };
        let Some(b) = object.bounds() else {
            continue;
        };
        let (dx, dy) = match mode {
            AlignMode::Left => (target_left - b.x, 0.0),
            AlignMode::CenterX => (target_center_x - b.center_x(), 0.0),
            AlignMode::Right => (target_right - b.right(), 0.0),
            AlignMode::Top => (0.0, target_top - b.y),
            AlignMode::CenterY => (0.0, target_center_y - b.center_y()),
            AlignMode::Bottom => (0.0, target_bottom - b.bottom()),
        };
        object.translate(dx, dy);
    }
}

/// Distribute the selected objects with equal gaps, keeping the outermost
/// objects fixed (L6).
pub fn distribute_objects(objects: &mut [LayoutObject], selection: &[usize], axis: DistributeAxis) {
    let mut items: Vec<(usize, ObjectBounds)> = selection
        .iter()
        .copied()
        .filter_map(|index| {
            objects
                .get(index)
                .and_then(|object| object.bounds())
                .map(|bounds| (index, bounds))
        })
        .collect();
    if items.len() < 3 {
        return;
    }
    match axis {
        DistributeAxis::Horizontal => items.sort_by(|a, b| {
            a.1.x
                .partial_cmp(&b.1.x)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        DistributeAxis::Vertical => items.sort_by(|a, b| {
            a.1.y
                .partial_cmp(&b.1.y)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
    }
    let first = items[0].1;
    let last = items[items.len() - 1].1;
    match axis {
        DistributeAxis::Horizontal => {
            let span = last.right() - first.x;
            let total_width: f32 = items.iter().map(|(_, b)| b.width).sum();
            let gap = (span - total_width) / (items.len() - 1) as f32;
            let mut cursor = first.x;
            for (index, b) in items {
                objects[index].translate(cursor - b.x, 0.0);
                cursor += b.width + gap;
            }
        }
        DistributeAxis::Vertical => {
            let span = last.bottom() - first.y;
            let total_height: f32 = items.iter().map(|(_, b)| b.height).sum();
            let gap = (span - total_height) / (items.len() - 1) as f32;
            let mut cursor = first.y;
            for (index, b) in items {
                objects[index].translate(0.0, cursor - b.y);
                cursor += b.height + gap;
            }
        }
    }
}

/// Bring an object to the top of the z-order (L6).
pub fn bring_to_front(objects: &mut Vec<LayoutObject>, index: usize) -> bool {
    let Some(last) = objects.len().checked_sub(1) else {
        return false;
    };
    move_object_layer(objects, index, last)
}

/// Send an object to the bottom of the z-order (L6).
pub fn send_to_back(objects: &mut Vec<LayoutObject>, index: usize) -> bool {
    move_object_layer(objects, index, 0)
}

/// Move an object one step up in the z-order (L6).
pub fn bring_forward(objects: &mut Vec<LayoutObject>, index: usize) -> bool {
    if index + 1 >= objects.len() {
        return false;
    }
    move_object_layer(objects, index, index + 1)
}

/// Move an object one step down in the z-order (L6).
pub fn send_backward(objects: &mut Vec<LayoutObject>, index: usize) -> bool {
    if index == 0 || index >= objects.len() {
        return false;
    }
    move_object_layer(objects, index, index - 1)
}

/// Move an object to an absolute z-order position (L6).
pub fn move_object_layer(objects: &mut Vec<LayoutObject>, from: usize, to: usize) -> bool {
    if from >= objects.len() {
        return false;
    }
    let object = objects.remove(from);
    let to = to.min(objects.len());
    objects.insert(to, object);
    true
}

/// Group the selected objects into one [`LayoutObject::Group`], preserving
/// their relative order (L6). Returns the group's index.
pub fn group_objects(
    objects: &mut Vec<LayoutObject>,
    selection: &[usize],
    name: &str,
) -> Option<usize> {
    let mut indices: Vec<usize> = selection
        .iter()
        .copied()
        .filter(|index| *index < objects.len())
        .collect();
    indices.sort_unstable();
    indices.dedup();
    if indices.is_empty() {
        return None;
    }
    let insert_at = indices.iter().copied().min().unwrap_or(0);
    let mut children = Vec::with_capacity(indices.len());
    for index in indices.iter().rev() {
        children.push(objects.remove(*index));
    }
    children.reverse();
    objects.insert(
        insert_at,
        LayoutObject::Group(LayoutGroup {
            name: name.to_string(),
            objects: children,
        }),
    );
    Some(insert_at)
}

/// Replace a group with its children at the same z-order position (L6).
pub fn ungroup_object(objects: &mut Vec<LayoutObject>, index: usize) -> bool {
    if !matches!(objects.get(index), Some(LayoutObject::Group(_))) {
        return false;
    }
    let LayoutObject::Group(group) = objects.remove(index) else {
        return false;
    };
    for (offset, child) in group.objects.into_iter().enumerate() {
        objects.insert(index + offset, child);
    }
    true
}

/// Orientation of a guide or page break (L6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideOrientation {
    Horizontal,
    Vertical,
}

/// An alignment guide drawn in the layout editor (L6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutGuide {
    pub orientation: GuideOrientation,
    pub position: f32,
}

impl LayoutGuide {
    pub fn new(orientation: GuideOrientation, position: f32) -> Self {
        Self {
            orientation,
            position,
        }
    }
}

/// A page break in the layout editor (L6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageBreak {
    pub position: f32,
}

/// Layout editor document: objects with lock state, guides, page breaks,
/// header/footer text and zoom (L6).
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutDocument {
    pub objects: Vec<LayoutObject>,
    /// Lock state parallel to `objects`.
    pub locked: Vec<bool>,
    pub guides: Vec<LayoutGuide>,
    pub page_breaks: Vec<PageBreak>,
    pub header: String,
    pub footer: String,
    /// Editor zoom factor (`1.0` = actual size).
    pub zoom: f32,
}

impl Default for LayoutDocument {
    fn default() -> Self {
        Self {
            objects: Vec::new(),
            locked: Vec::new(),
            guides: Vec::new(),
            page_breaks: Vec::new(),
            header: String::new(),
            footer: String::new(),
            zoom: 1.0,
        }
    }
}

impl LayoutDocument {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an object and return its index.
    pub fn add(&mut self, object: LayoutObject) -> usize {
        self.objects.push(object);
        self.locked.push(false);
        self.objects.len() - 1
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn is_locked(&self, index: usize) -> bool {
        self.locked.get(index).copied().unwrap_or(false)
    }

    pub fn lock(&mut self, index: usize) -> bool {
        if index >= self.objects.len() {
            return false;
        }
        self.locked[index] = true;
        true
    }

    pub fn unlock(&mut self, index: usize) -> bool {
        if index >= self.objects.len() {
            return false;
        }
        self.locked[index] = false;
        true
    }

    pub fn toggle_lock(&mut self, index: usize) -> bool {
        if index >= self.objects.len() {
            return false;
        }
        self.locked[index] = !self.locked[index];
        true
    }

    fn unlocked_selection(&self, selection: &[usize]) -> Vec<usize> {
        selection
            .iter()
            .copied()
            .filter(|index| *index < self.objects.len() && !self.is_locked(*index))
            .collect()
    }

    /// Align unlocked selected objects; returns how many objects moved (L6).
    /// Locked objects still contribute to the alignment target but do not move.
    pub fn align(&mut self, selection: &[usize], mode: AlignMode) -> usize {
        let reference: Vec<usize> = selection
            .iter()
            .copied()
            .filter(|index| *index < self.objects.len())
            .collect();
        let movable = self.unlocked_selection(selection);
        align_objects_with_reference(&mut self.objects, &reference, &movable, mode);
        movable.len()
    }

    /// Distribute unlocked selected objects; returns how many were eligible
    /// (L6).
    pub fn distribute(&mut self, selection: &[usize], axis: DistributeAxis) -> usize {
        let indices = self.unlocked_selection(selection);
        distribute_objects(&mut self.objects, &indices, axis);
        indices.len()
    }

    fn move_layer(&mut self, from: usize, to: usize) -> bool {
        if from >= self.objects.len() || self.is_locked(from) {
            return false;
        }
        let object = self.objects.remove(from);
        let lock = self.locked.remove(from);
        let to = to.min(self.objects.len());
        self.objects.insert(to, object);
        self.locked.insert(to, lock);
        true
    }

    pub fn bring_to_front(&mut self, index: usize) -> bool {
        let Some(last) = self.objects.len().checked_sub(1) else {
            return false;
        };
        self.move_layer(index, last)
    }

    pub fn send_to_back(&mut self, index: usize) -> bool {
        self.move_layer(index, 0)
    }

    pub fn bring_forward(&mut self, index: usize) -> bool {
        if index + 1 >= self.objects.len() {
            return false;
        }
        self.move_layer(index, index + 1)
    }

    pub fn send_backward(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.objects.len() {
            return false;
        }
        self.move_layer(index, index - 1)
    }

    /// Group unlocked selected objects (L6).
    pub fn group(&mut self, selection: &[usize], name: &str) -> Option<usize> {
        let indices = self.unlocked_selection(selection);
        if indices.is_empty() {
            return None;
        }
        let insert_at = indices.iter().copied().min().unwrap_or(0);
        let group = group_objects(&mut self.objects, &indices, name)?;
        let mut indices = indices;
        indices.sort_unstable();
        indices.dedup();
        for index in indices.iter().rev() {
            if *index < self.locked.len() {
                self.locked.remove(*index);
            }
        }
        self.locked.insert(insert_at.min(self.locked.len()), false);
        Some(group)
    }

    /// Ungroup the group at `index` (L6).
    pub fn ungroup(&mut self, index: usize) -> bool {
        if !matches!(self.objects.get(index), Some(LayoutObject::Group(_))) || self.is_locked(index)
        {
            return false;
        }
        let child_count = match &self.objects[index] {
            LayoutObject::Group(group) => group.objects.len(),
            _ => 0,
        };
        if !ungroup_object(&mut self.objects, index) {
            return false;
        }
        if index < self.locked.len() {
            self.locked.remove(index);
        }
        for offset in 0..child_count {
            self.locked.insert(index + offset, false);
        }
        true
    }

    /// Add an alignment guide and return its index (L6).
    pub fn add_guide(&mut self, orientation: GuideOrientation, position: f32) -> usize {
        self.guides.push(LayoutGuide::new(orientation, position));
        self.guides.len() - 1
    }

    pub fn clear_guides(&mut self) {
        self.guides.clear();
    }

    /// Add a horizontal page break and return its index (L6).
    pub fn add_page_break(&mut self, position: f32) -> usize {
        self.page_breaks.push(PageBreak { position });
        self.page_breaks.len() - 1
    }

    pub fn clear_page_breaks(&mut self) {
        self.page_breaks.clear();
    }

    pub fn set_header(&mut self, header: impl Into<String>) {
        self.header = header.into();
    }

    pub fn set_footer(&mut self, footer: impl Into<String>) {
        self.footer = footer.into();
    }

    /// Union of all object bounds (L6).
    pub fn content_bounds(&self) -> Option<ObjectBounds> {
        let mut bounds: Option<ObjectBounds> = None;
        for object in &self.objects {
            if let Some(child_bounds) = object.bounds() {
                bounds = Some(match bounds {
                    Some(current) => {
                        let x = current.x.min(child_bounds.x);
                        let y = current.y.min(child_bounds.y);
                        let right = current.right().max(child_bounds.right());
                        let bottom = current.bottom().max(child_bounds.bottom());
                        ObjectBounds {
                            x,
                            y,
                            width: right - x,
                            height: bottom - y,
                        }
                    }
                    None => child_bounds,
                });
            }
        }
        bounds
    }

    /// Zoom factor that fits the document content on a page (L6).
    pub fn fit_zoom(&self, page_width: f32, page_height: f32) -> f32 {
        let Some(bounds) = self.content_bounds() else {
            return 1.0;
        };
        if page_width <= 0.0 || page_height <= 0.0 || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return 1.0;
        }
        (page_width / bounds.width)
            .min(page_height / bounds.height)
            .clamp(0.1, 5.0)
    }

    /// Set the zoom factor (L6).
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.max(0.1);
    }

    /// Return to 100% zoom (L6).
    pub fn actual_size(&mut self) {
        self.zoom = 1.0;
    }

    /// Fit the document to a page and store the resulting zoom (L6).
    pub fn fit_to_page(&mut self, page_width: f32, page_height: f32) -> f32 {
        self.zoom = self.fit_zoom(page_width, page_height);
        self.zoom
    }

    /// SVG markup for the alignment guides and page breaks (L6).
    pub fn guides_svg(&self, page_width: f32, page_height: f32) -> String {
        let mut svg = String::new();
        for guide in &self.guides {
            match guide.orientation {
                GuideOrientation::Horizontal => {
                    svg.push_str(&format!(
                        "<line x1=\"0\" y1=\"{:.1}\" x2=\"{page_width:.1}\" y2=\"{:.1}\" stroke=\"#3b82f6\" stroke-width=\"1\" stroke-dasharray=\"4 3\"/>",
                        guide.position, guide.position
                    ));
                }
                GuideOrientation::Vertical => {
                    svg.push_str(&format!(
                        "<line x1=\"{:.1}\" y1=\"0\" x2=\"{:.1}\" y2=\"{page_height:.1}\" stroke=\"#3b82f6\" stroke-width=\"1\" stroke-dasharray=\"4 3\"/>",
                        guide.position, guide.position
                    ));
                }
            }
        }
        for page_break in &self.page_breaks {
            svg.push_str(&format!(
                "<line x1=\"0\" y1=\"{:.1}\" x2=\"{page_width:.1}\" y2=\"{:.1}\" stroke=\"#ef4444\" stroke-width=\"1\" stroke-dasharray=\"8 4\"/>",
                page_break.position, page_break.position
            ));
        }
        svg
    }

    /// SVG markup for the edited header/footer text (L6).
    pub fn header_footer_svg(&self, page_width: f32, page_height: f32) -> String {
        let mut svg = String::new();
        if !self.header.is_empty() {
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"18.0\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"12\" fill=\"#222222\">{}</text>",
                page_width / 2.0,
                svg_escape(&self.header)
            ));
        }
        if !self.footer.is_empty() {
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"12\" fill=\"#222222\">{}</text>",
                page_width / 2.0,
                page_height - 8.0,
                svg_escape(&self.footer)
            ));
        }
        svg
    }
}

/// Tick positions for an editor ruler of `length` with `spacing` between major
/// ticks (L6).
pub fn ruler_ticks(length: f32, spacing: f32) -> Vec<f32> {
    if length <= 0.0 || spacing <= 0.0 {
        return Vec::new();
    }
    let mut ticks = Vec::new();
    let mut value = 0.0;
    while value <= length + 1e-6 {
        ticks.push(value);
        value += spacing;
    }
    ticks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::{PlotStyle, dot_plot_svg};
    use crate::statistics::Statistic;
    use ndarray::array;

    #[test]
    fn render_layout_inlines_panels() {
        let data = array![[1.0, 2.0], [3.0, 4.0]];
        let panel = dot_plot_svg(
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
        );
        let panels = vec![
            LayoutPanel {
                title: "Sample 1".into(),
                svg: panel.clone(),
            },
            LayoutPanel {
                title: "Sample 2".into(),
                svg: panel,
            },
        ];
        let out = render_layout(&panels, &LayoutOptions::default());
        assert!(out.starts_with("<svg"));
        assert!(out.contains("Sample 1"));
        assert!(out.contains("Sample 2"));
        // A single outer <svg>; panel content is inlined into <g> groups.
        assert_eq!(out.matches("<svg").count(), 1);
        assert_eq!(out.matches("<g ").count(), 2);
        // Titles carry a font family so raster export picks a sane font.
        assert!(out.contains("font-family=\"sans-serif\""));
        assert!(!out.contains("<svg x="));
    }

    #[test]
    fn render_empty_layout() {
        let out = render_layout(&[], &LayoutOptions::default());
        assert!(out.starts_with("<svg"));
        assert!(out.ends_with("</svg>\n"));
    }

    #[test]
    fn render_free_layout_scales_and_places() {
        let data = array![[1.0, 2.0], [3.0, 4.0]];
        let panel = dot_plot_svg(
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
        );
        let placements = vec![LayoutPlacement {
            title: "Sample 1".into(),
            svg: panel,
            svg_width: 500,
            svg_height: 450,
            x: 30.0,
            y: 40.0,
            width: 300.0,
            height: 260.0,
            title_size: 13.0,
        }];
        let out = render_layout_free(800, 600, &placements, &LayoutOptions::default());
        assert!(out.starts_with("<svg"));
        assert!(out.contains("translate(30.00,68.00)"));
        assert!(out.contains("scale(0.6000,0.5778)"));
        assert!(out.contains("Sample 1"));
        assert_eq!(out.matches("<svg").count(), 1);
    }

    #[test]
    fn resolve_fjml_replaces_template_tokens() {
        let ctx = FjmlContext::new(
            "SampleA",
            Some("CD4_Median"),
            Some("reagent-x"),
            "2026-11-14",
        );
        assert_eq!(
            resolve_fjml(
                "Sample {{sample.name}} {{stat:CD4_Median}} {{keyword:reagent-x}} {{date}}",
                &ctx
            ),
            "Sample SampleA CD4_Median reagent-x 2026-11-14"
        );
        assert_eq!(resolve_fjml("no tokens here", &ctx), "no tokens here");
    }

    #[test]
    fn layout_object_svg_renders_resolved_text() {
        let text = LayoutText {
            content: "Sample {{sample.name}}".into(),
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 20.0,
            font_size: 12.0,
            color: "#000000".into(),
            opacity: 1.0,
            rotation_deg: 0.0,
            align: TextAlign::Start,
        };
        let obj = LayoutObject::Text(text);
        let ctx = FjmlContext::new("SampleA", None, None, "2026-11-14");
        let svg = layout_object_svg(&obj, &ctx, &LayoutOptions::default());
        assert!(svg.contains("Sample SampleA"));
        assert!(svg.contains("<text"));
    }

    #[test]
    fn layout_object_svg_renders_rotated_text() {
        let text = LayoutText {
            content: "rotated".into(),
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 20.0,
            font_size: 12.0,
            color: "#000000".into(),
            opacity: 1.0,
            rotation_deg: 45.0,
            align: TextAlign::Center,
        };
        let obj = LayoutObject::Text(text);
        let ctx = FjmlContext::new("SampleA", None, None, "2026-11-14");
        let svg = layout_object_svg(&obj, &ctx, &LayoutOptions::default());
        assert!(svg.contains("rotate(45.00"));
    }

    #[test]
    fn layout_object_svg_renders_rect_shape() {
        let shape = LayoutShape {
            kind: ShapeKind::Rect,
            x: 1.0,
            y: 2.0,
            width: 10.0,
            height: 20.0,
            fill: Some("#ff0000".into()),
            stroke: None,
            stroke_width: 1.0,
            opacity: 0.5,
            shadow: false,
            rotation_deg: 0.0,
        };
        let obj = LayoutObject::Shape(shape);
        let ctx = FjmlContext::new("SampleA", None, None, "2026-11-14");
        let svg = layout_object_svg(&obj, &ctx, &LayoutOptions::default());
        assert!(svg.contains("<rect x=\"1.0\""));
        assert!(svg.contains("fill=\"#ff0000\""));
    }

    #[test]
    fn layout_object_svg_renders_group_with_child() {
        let group = LayoutGroup {
            name: "group-a".into(),
            objects: vec![LayoutObject::Text(LayoutText {
                content: "child".into(),
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 10.0,
                font_size: 10.0,
                color: "#000000".into(),
                opacity: 1.0,
                rotation_deg: 0.0,
                align: TextAlign::Start,
            })],
        };
        let obj = LayoutObject::Group(group);
        let ctx = FjmlContext::new("SampleA", None, None, "2026-11-14");
        let svg = layout_object_svg(&obj, &ctx, &LayoutOptions::default());
        assert!(svg.contains("child"));
        assert!(svg.contains("group-a"));
    }

    #[test]
    fn layout_objects_to_svg_renders_objects_and_respects_transparency() {
        let obj = LayoutObject::Text(LayoutText {
            content: "hello".into(),
            x: 5.0,
            y: 5.0,
            width: 50.0,
            height: 10.0,
            font_size: 10.0,
            color: "#000000".into(),
            opacity: 1.0,
            rotation_deg: 0.0,
            align: TextAlign::Start,
        });
        let svg = layout_objects_to_svg(
            std::slice::from_ref(&obj),
            &FjmlContext::new("", None, None, ""),
            &LayoutOptions::default(),
            800,
            600,
            false,
        );
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("hello"));
        assert!(svg.contains("<rect x=\"0\""));

        let svg_t = layout_objects_to_svg(
            &[obj],
            &FjmlContext::new("", None, None, ""),
            &LayoutOptions::default(),
            800,
            600,
            true,
        );
        assert!(svg_t.starts_with("<svg"));
        assert!(svg_t.contains("hello"));
        assert!(!svg_t.contains("<rect x=\"0\""));
    }

    #[test]
    fn render_grid_layout_renders_cells_and_axis_labels() {
        let grid = LayoutGrid {
            columns: 2,
            rows: 2,
            column_width: 100.0,
            row_height: 50.0,
            column_widths: Vec::new(),
            row_heights: Vec::new(),
            merges: Vec::new(),
            padding: 5.0,
            border: true,
            axis_labels: true,
        };
        let svg = render_grid_layout(&grid, &LayoutOptions::default());
        assert!(svg.contains("<svg"));
        assert!(svg.contains("columns=2"));
        assert!(svg.contains("rows=2"));
        assert!(svg.contains("<rect"));
    }

    #[test]
    fn grid_insert_delete_tracks_and_merge_shifts() {
        let mut grid = LayoutGrid::default();
        assert!(grid_merge_cells(&mut grid, 0, 0, 2, 2));
        assert!(grid.is_merged(1, 1));

        grid_insert_row(&mut grid, 1);
        assert_eq!(grid.rows, 3);
        assert_eq!(grid.row_sizes(), vec![450.0, 450.0, 450.0]);
        let merge = grid.merges[0];
        assert_eq!((merge.row, merge.row_span), (0, 3));

        grid_delete_row(&mut grid, 1);
        assert_eq!(grid.rows, 2);
        let merge = grid.merges[0];
        assert_eq!((merge.row, merge.row_span), (0, 2));

        grid_insert_column(&mut grid, 0);
        assert_eq!(grid.columns, 3);
        let merge = grid.merges[0];
        assert_eq!((merge.column, merge.column_span), (1, 2));

        grid_delete_column(&mut grid, 0);
        assert_eq!(grid.columns, 2);
        let merge = grid.merges[0];
        assert_eq!((merge.column, merge.column_span), (0, 2));
    }

    #[test]
    fn grid_merge_and_split_cells() {
        let mut grid = LayoutGrid::default();
        assert!(grid_merge_cells(&mut grid, 0, 0, 1, 2));
        assert!(grid.is_merged(0, 1));
        // Overlap and single-cell merges are rejected.
        assert!(!grid_merge_cells(&mut grid, 0, 0, 1, 1));
        assert!(!grid_merge_cells(&mut grid, 0, 1, 1, 1));
        assert!(!grid_merge_cells(&mut grid, 0, 0, 2, 2));
        // A disjoint merge is accepted.
        assert!(grid_merge_cells(&mut grid, 1, 0, 1, 2));
        assert_eq!(grid.merges.len(), 2);
        assert!(grid_split_cell(&mut grid, 0, 1));
        assert!(!grid.is_merged(0, 0));
        assert!(grid.is_merged(1, 1));
        assert!(!grid_split_cell(&mut grid, 0, 0));
        assert_eq!(grid.merges.len(), 1);
    }

    #[test]
    fn grid_autofit_and_equal_spacing() {
        let mut grid = LayoutGrid {
            column_widths: vec![400.0, 800.0],
            row_heights: vec![300.0, 900.0],
            padding: 10.0,
            ..Default::default()
        };
        let scale = grid_autofit(&mut grid, 610.0, 500.0);
        assert!(scale > 0.0 && scale < 1.0);
        let widths = grid.column_sizes();
        let heights = grid.row_sizes();
        let total_w = widths.iter().sum::<f32>() + grid.padding * 3.0;
        let total_h = heights.iter().sum::<f32>() + grid.padding * 3.0;
        assert!(total_w <= 610.0 + 0.01);
        assert!(total_h <= 500.0 + 0.01);

        let mut grid = LayoutGrid {
            column_widths: vec![100.0, 300.0],
            row_heights: vec![50.0, 150.0],
            ..Default::default()
        };
        grid_equal_spacing(&mut grid);
        assert_eq!(grid.column_sizes(), vec![300.0, 300.0]);
        assert_eq!(grid.row_sizes(), vec![150.0, 150.0]);
    }

    #[test]
    fn render_grid_layout_merges_cells_and_labels_edges() {
        let mut grid = LayoutGrid {
            columns: 3,
            rows: 2,
            column_width: 100.0,
            row_height: 50.0,
            column_widths: Vec::new(),
            row_heights: Vec::new(),
            merges: Vec::new(),
            padding: 5.0,
            border: false,
            axis_labels: true,
        };
        assert!(grid_merge_cells(&mut grid, 0, 0, 2, 2));
        let svg = render_grid_layout(&grid, &LayoutOptions::default());
        // 3x2 grid with a 2x2 merge -> 1 merged rectangle + 2 remaining cells.
        assert_eq!(svg.matches("<rect").count(), 3);
        // Edge labels: column letters across the top, row numbers down the
        // left, plus the grid summary.
        assert!(svg.contains(">A</text>"));
        assert!(svg.contains(">B</text>"));
        assert!(svg.contains(">C</text>"));
        assert!(svg.contains(">1</text>"));
        assert!(svg.contains(">2</text>"));
        assert!(svg.contains("columns=3"));
        assert!(svg.contains("rows=2"));
        // The merged rectangle spans both columns and both rows.
        assert!(svg.contains("width=\"205.0\""));
        assert!(svg.contains("height=\"105.0\""));
    }

    #[test]
    fn layout_object_svg_renders_table_with_header_and_rows() {
        let table = LayoutObject::Table(LayoutTable::new(
            0.0,
            0.0,
            600.0,
            100.0,
            "Proliferation",
            vec!["generation".into(), "peak".into()],
            vec![vec!["1".into(), "10".into()], vec!["2".into(), "20".into()]],
        ));
        let ctx = FjmlContext::new("", None, None, "");
        let svg = layout_object_svg(&table, &ctx, &LayoutOptions::default());
        assert!(svg.contains("Proliferation"));
        assert!(svg.contains("generation"));
        assert!(svg.contains("peak"));
        assert!(svg.contains(">20<"));
    }

    #[test]
    fn layout_object_svg_renders_table_heat_flags_flagged_rows() {
        let heat = HeatRange {
            min: 0.0,
            max: 10.0,
            palette: "viridis".into(),
        };
        let table = LayoutObject::Table(
            LayoutTable::new(
                0.0,
                0.0,
                600.0,
                100.0,
                "Diff",
                vec!["param".into(), "difference".into()],
                vec![vec!["A".into(), "15".into()]],
            )
            .with_heat(heat),
        );
        let ctx = FjmlContext::new("", None, None, "");
        let svg = layout_object_svg(&table, &ctx, &LayoutOptions::default());
        // Flagged (|15| > heat max 10) -> heat color fill.
        assert!(svg.contains("#ef442b"));
    }

    #[test]
    fn layout_object_difference_table_builds_flagged_table() {
        let rows = vec![DifferenceRow::new("APC", Statistic::Mean, 12.0, 8.0)];
        let obj = LayoutObject::difference_table(
            "Diff vs control",
            &rows,
            Some(HeatRange {
                min: 0.0,
                max: 10.0,
                palette: "viridis".into(),
            }),
        );
        let ctx = FjmlContext::new("", None, None, "");
        let svg = layout_object_svg(&obj, &ctx, &LayoutOptions::default());
        assert!(svg.contains("Diff vs control"));
        assert!(svg.contains("APC"));
        assert!(svg.contains("mean"));
    }

    #[test]
    fn layout_object_svg_renders_legend_with_swatches() {
        let legend = LayoutObject::Legend(LayoutLegend::new(
            0.0,
            0.0,
            200.0,
            40.0,
            "Heat",
            vec![
                LayoutLegendEntry {
                    label: "High".into(),
                    color: "#ef4444".into(),
                },
                LayoutLegendEntry {
                    label: "Low".into(),
                    color: "#3b82f6".into(),
                },
            ],
        ));
        let ctx = FjmlContext::new("", None, None, "");
        let svg = layout_object_svg(&legend, &ctx, &LayoutOptions::default());
        assert!(svg.contains("Heat"));
        assert!(svg.contains("High"));
        assert!(svg.contains("Low"));
        assert!(svg.contains("#ef4444"));
        assert!(svg.contains("#3b82f6"));
    }

    #[test]
    fn layout_object_svg_renders_heat_map_cells() {
        let map = LayoutObject::HeatMap(LayoutHeatMap {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 100.0,
            title: "Plate".into(),
            row_labels: vec!["W1".into()],
            col_labels: vec!["A1".into(), "A2".into()],
            values: vec![vec![0.2, 0.8]],
            heat: Some(HeatRange {
                min: 0.0,
                max: 1.0,
                palette: "viridis".into(),
            }),
        });
        let ctx = FjmlContext::new("", None, None, "");
        let svg = layout_object_svg(&map, &ctx, &LayoutOptions::default());
        assert!(svg.contains("Plate"));
        assert!(svg.contains("W1"));
        assert!(svg.contains("A1"));
        assert!(svg.contains("<rect"));
    }

    fn test_shape(x: f32, y: f32, width: f32, height: f32) -> LayoutObject {
        LayoutObject::Shape(LayoutShape {
            kind: ShapeKind::Rect,
            x,
            y,
            width,
            height,
            fill: None,
            stroke: None,
            stroke_width: 1.0,
            opacity: 1.0,
            shadow: false,
            rotation_deg: 0.0,
        })
    }

    #[test]
    fn align_objects_aligns_edges_and_centers() {
        let mut objects = vec![
            test_shape(10.0, 20.0, 20.0, 10.0),
            test_shape(100.0, 50.0, 30.0, 20.0),
            test_shape(50.0, 80.0, 10.0, 30.0),
        ];
        align_objects(&mut objects, &[0, 1, 2], AlignMode::Left);
        for object in &objects {
            assert_eq!(object.bounds().unwrap().x, 10.0);
        }

        let mut objects = vec![
            test_shape(10.0, 20.0, 20.0, 10.0),
            test_shape(100.0, 50.0, 30.0, 20.0),
            test_shape(50.0, 80.0, 10.0, 30.0),
        ];
        align_objects(&mut objects, &[0, 1, 2], AlignMode::Right);
        for object in &objects {
            assert!((object.bounds().unwrap().right() - 130.0).abs() < 1e-6);
        }

        let mut objects = vec![
            test_shape(10.0, 20.0, 20.0, 10.0),
            test_shape(100.0, 50.0, 30.0, 20.0),
        ];
        align_objects(&mut objects, &[0, 1], AlignMode::CenterX);
        let first = objects[0].bounds().unwrap().center_x();
        let second = objects[1].bounds().unwrap().center_x();
        assert!((first - second).abs() < 1e-6);

        align_objects(&mut objects, &[0, 1], AlignMode::Top);
        assert_eq!(
            objects[0].bounds().unwrap().y,
            objects[1].bounds().unwrap().y
        );
        align_objects(&mut objects, &[0, 1], AlignMode::Bottom);
        assert!(
            (objects[0].bounds().unwrap().bottom() - objects[1].bounds().unwrap().bottom()).abs()
                < 1e-6
        );
        align_objects(&mut objects, &[0, 1], AlignMode::CenterY);
        assert!(
            (objects[0].bounds().unwrap().center_y() - objects[1].bounds().unwrap().center_y())
                .abs()
                < 1e-6
        );
    }

    #[test]
    fn distribute_objects_equalizes_gaps() {
        let mut objects = vec![
            test_shape(0.0, 0.0, 10.0, 10.0),
            test_shape(20.0, 0.0, 10.0, 10.0),
            test_shape(100.0, 0.0, 10.0, 10.0),
        ];
        distribute_objects(&mut objects, &[0, 1, 2], DistributeAxis::Horizontal);
        let xs: Vec<f32> = objects
            .iter()
            .map(|object| object.bounds().unwrap().x)
            .collect();
        assert_eq!(xs[0], 0.0);
        assert_eq!(xs[2], 100.0);
        let gap_a = xs[1] - (xs[0] + 10.0);
        let gap_b = xs[2] - (xs[1] + 10.0);
        assert!((gap_a - gap_b).abs() < 1e-6);

        let mut objects = vec![
            test_shape(0.0, 0.0, 10.0, 10.0),
            test_shape(0.0, 20.0, 10.0, 10.0),
            test_shape(0.0, 100.0, 10.0, 10.0),
        ];
        distribute_objects(&mut objects, &[0, 1, 2], DistributeAxis::Vertical);
        let ys: Vec<f32> = objects
            .iter()
            .map(|object| object.bounds().unwrap().y)
            .collect();
        assert_eq!(ys[0], 0.0);
        assert_eq!(ys[2], 100.0);
        assert!(((ys[1] - 10.0) - (ys[2] - (ys[1] + 10.0))).abs() < 1e-6);
    }

    #[test]
    fn layer_and_group_ungroup_objects() {
        let mut objects = vec![
            test_shape(0.0, 0.0, 10.0, 10.0),
            test_shape(20.0, 0.0, 10.0, 10.0),
            test_shape(40.0, 0.0, 10.0, 10.0),
        ];
        assert!(bring_to_front(&mut objects, 0));
        assert_eq!(objects[2].bounds().unwrap().x, 0.0);
        assert!(send_to_back(&mut objects, 2));
        assert_eq!(objects[0].bounds().unwrap().x, 0.0);
        assert!(bring_forward(&mut objects, 0));
        assert_eq!(objects[1].bounds().unwrap().x, 0.0);
        assert!(send_backward(&mut objects, 1));
        assert_eq!(objects[0].bounds().unwrap().x, 0.0);
        assert!(!bring_forward(&mut objects, 2));

        let group_index = group_objects(&mut objects, &[0, 2], "pair").unwrap();
        assert_eq!(objects.len(), 2);
        assert!(matches!(objects[group_index], LayoutObject::Group(_)));
        let bounds = objects[group_index].bounds().unwrap();
        assert_eq!((bounds.x, bounds.width), (0.0, 50.0));
        assert!(ungroup_object(&mut objects, group_index));
        assert_eq!(objects.len(), 3);
        assert!(!ungroup_object(&mut objects, 0));
        assert!(!group_objects(&mut objects, &[], "empty").is_some());
    }

    #[test]
    fn layout_document_locks_guides_page_breaks_and_zoom() {
        let mut document = LayoutDocument::new();
        document.add(test_shape(10.0, 20.0, 90.0, 30.0));
        document.add(test_shape(200.0, 100.0, 100.0, 50.0));
        assert_eq!(document.len(), 2);
        assert!(document.lock(0));
        assert!(document.is_locked(0));
        assert!(document.toggle_lock(0));
        assert!(!document.is_locked(0));
        assert!(document.toggle_lock(0));
        assert!(!document.lock(9));

        // Locked objects do not move during alignment.
        assert_eq!(document.align(&[0, 1], AlignMode::Left), 1);
        assert_eq!(document.objects[0].bounds().unwrap().x, 10.0);
        assert_eq!(document.objects[1].bounds().unwrap().x, 10.0);
        assert_eq!(document.distribute(&[0, 1], DistributeAxis::Horizontal), 1);

        // Layering honors locks.
        assert!(!document.send_to_back(0));
        assert!(document.send_to_back(1));

        // Grouping skips locked members and keeps the lock vector in sync.
        assert!(document.unlock(0));
        assert!(document.unlock(1));
        let group = document.group(&[0, 1], "all").unwrap();
        assert_eq!(document.objects.len(), 1);
        assert_eq!(document.locked.len(), 1);
        assert!(document.ungroup(group));
        assert_eq!(document.objects.len(), 2);
        assert_eq!(document.locked.len(), 2);

        assert_eq!(document.add_guide(GuideOrientation::Horizontal, 40.0), 0);
        assert_eq!(document.add_guide(GuideOrientation::Vertical, 80.0), 1);
        let guides = document.guides_svg(400.0, 300.0);
        assert!(guides.contains("stroke-dasharray"));
        assert_eq!(document.add_page_break(200.0), 0);
        assert!(document.guides_svg(400.0, 300.0).contains("#ef4444"));
        document.clear_guides();
        assert!(document.guides.is_empty());

        document.set_header("Header");
        document.set_footer("Footer");
        let text = document.header_footer_svg(400.0, 300.0);
        assert!(text.contains(">Header<"));
        assert!(text.contains(">Footer<"));

        document.zoom = 2.0;
        document.actual_size();
        assert_eq!(document.zoom, 1.0);
        document.set_zoom(0.0);
        assert_eq!(document.zoom, 0.1);

        let bounds = document.content_bounds().unwrap();
        assert_eq!((bounds.width, bounds.height), (100.0, 130.0));
        let fit = document.fit_to_page(580.0, 260.0);
        assert!((fit - 2.0).abs() < 1e-6);
        assert_eq!(document.fit_zoom(0.0, 0.0), 1.0);
    }

    #[test]
    fn ruler_ticks_and_header_footer_svg() {
        assert_eq!(ruler_ticks(100.0, 25.0), vec![0.0, 25.0, 50.0, 75.0, 100.0]);
        assert!(ruler_ticks(0.0, 10.0).is_empty());
        assert!(ruler_ticks(100.0, 0.0).is_empty());

        let mut document = LayoutDocument::new();
        document.set_header("A&B");
        let svg = document.header_footer_svg(200.0, 100.0);
        assert!(svg.contains("A&amp;B"));
    }
}
