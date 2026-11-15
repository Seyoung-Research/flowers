//! 3D point-cloud, bubble, terrain and voxel rendering (V3).
//!
//! The module is deliberately dependency-free: it projects 3D points with a
//! small yaw/pitch camera and writes SVG, so both the GUI (through
//! [`projected_points`]) and headless exports share one deterministic camera
//! model.

use crate::plot::{DensityMap, density_colour};

/// Orbit camera for 3D views.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Rotation around the vertical (Y) axis, in degrees.
    pub yaw_deg: f64,
    /// Rotation around the horizontal (X) axis, in degrees.
    pub pitch_deg: f64,
    /// Zoom factor (`1.0` fits the unit cube).
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw_deg: 35.0,
            pitch_deg: 22.0,
            zoom: 1.0,
        }
    }
}

impl Camera {
    /// Rotates a world-space point into camera space.
    pub fn rotate(&self, p: [f64; 3]) -> [f64; 3] {
        let yaw = self.yaw_deg.to_radians();
        let pitch = self.pitch_deg.to_radians();
        let (sy, cy) = yaw.sin_cos();
        let x1 = p[0] * cy + p[2] * sy;
        let z1 = -p[0] * sy + p[2] * cy;
        let (sp, cp) = pitch.sin_cos();
        let y2 = p[1] * cp - z1 * sp;
        let z2 = p[1] * sp + z1 * cp;
        [x1, y2, z2]
    }
}

/// A projected point: screen position plus camera-space depth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projected {
    /// Screen coordinates in SVG units.
    pub screen: [f64; 2],
    /// Camera-space depth (larger is closer to the viewer).
    pub depth: f64,
}

/// Projects a world-space point into screen space with a mild perspective.
pub fn project_point(p: [f64; 3], camera: &Camera, width: u32, height: u32) -> Projected {
    let r = camera.rotate(p);
    let scale = width.min(height) as f64 * 0.4 * camera.zoom.max(0.05);
    let persp = 1.0 / (1.0 + (r[2] * 0.28).max(-0.8));
    Projected {
        screen: [
            width as f64 / 2.0 + r[0] * scale * persp,
            height as f64 / 2.0 - r[1] * scale * persp,
        ],
        depth: r[2],
    }
}

/// How a 3D dataset is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View3DMode {
    /// One dot per event.
    #[default]
    PointCloud,
    /// One bubble per event with a size parameter.
    Bubble,
    /// A terrain surface built from a 2D density grid.
    Terrain,
    /// Occupied cells of a 3D voxel grid.
    Voxel,
}

impl View3DMode {
    /// All modes, in display order.
    pub fn all() -> [Self; 4] {
        [
            View3DMode::PointCloud,
            View3DMode::Bubble,
            View3DMode::Terrain,
            View3DMode::Voxel,
        ]
    }

    /// Stable identifier used by project files.
    pub fn id(self) -> &'static str {
        match self {
            View3DMode::PointCloud => "point-cloud",
            View3DMode::Bubble => "bubble",
            View3DMode::Terrain => "terrain",
            View3DMode::Voxel => "voxel",
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            View3DMode::PointCloud => "Point cloud",
            View3DMode::Bubble => "Bubble",
            View3DMode::Terrain => "Terrain",
            View3DMode::Voxel => "Voxel",
        }
    }

    /// Parses [`Self::id`].
    pub fn from_id(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "point-cloud" | "point_cloud" | "points" => Some(View3DMode::PointCloud),
            "bubble" => Some(View3DMode::Bubble),
            "terrain" => Some(View3DMode::Terrain),
            "voxel" => Some(View3DMode::Voxel),
            _ => None,
        }
    }
}

/// Configuration for a 3D view.
#[derive(Debug, Clone)]
pub struct Spec3D {
    pub x: usize,
    pub y: usize,
    pub z: usize,
    /// Optional parameter mapped to colour.
    pub color: Option<usize>,
    /// Optional parameter mapped to bubble size.
    pub size: Option<usize>,
    pub mode: View3DMode,
    pub camera: Camera,
    pub density_map: DensityMap,
    /// Maximum number of events sampled into the point cloud.
    pub max_points: usize,
    /// Grid resolution for the voxel mode.
    pub voxel_grid: usize,
    /// Grid resolution for the terrain mode.
    pub terrain_grid: usize,
}

impl Default for Spec3D {
    fn default() -> Self {
        Self {
            x: 0,
            y: 1,
            z: 2,
            color: None,
            size: None,
            mode: View3DMode::PointCloud,
            camera: Camera::default(),
            density_map: DensityMap::Rainbow,
            max_points: 20_000,
            voxel_grid: 16,
            terrain_grid: 32,
        }
    }
}

/// One rendered 3D event.
#[derive(Debug, Clone, PartialEq)]
pub struct Point3 {
    /// Normalized position in `[-1, 1]` on each axis.
    pub pos: [f64; 3],
    /// Bubble/dot colour.
    pub color: (u8, u8, u8),
    /// Relative radius multiplier.
    pub size: f64,
}

fn axis_range(data: &ndarray::Array2<f32>, col: usize) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for row in 0..data.nrows() {
        let v = data[[row, col]] as f64;
        if v.is_finite() {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    if !lo.is_finite() || !hi.is_finite() || (hi - lo).abs() < f64::EPSILON {
        (0.0, 1.0)
    } else {
        (lo, hi)
    }
}

fn norm01(v: f64, range: (f64, f64)) -> f64 {
    if (range.1 - range.0).abs() < f64::EPSILON {
        0.5
    } else {
        ((v - range.0) / (range.1 - range.0)).clamp(0.0, 1.0)
    }
}

fn colour_u8(c: (f64, f64, f64)) -> (u8, u8, u8) {
    (
        (c.0 * 255.0).round() as u8,
        (c.1 * 255.0).round() as u8,
        (c.2 * 255.0).round() as u8,
    )
}

/// Builds normalized 3D points from event data according to `spec`.
pub fn build_points(data: &ndarray::Array2<f32>, spec: &Spec3D) -> Vec<Point3> {
    let (n, p) = data.dim();
    if n == 0 || spec.x >= p || spec.y >= p || spec.z >= p {
        return Vec::new();
    }
    let xr = axis_range(data, spec.x);
    let yr = axis_range(data, spec.y);
    let zr = axis_range(data, spec.z);
    let cr = spec.color.filter(|&c| c < p).map(|c| axis_range(data, c));
    let sr = spec.size.filter(|&c| c < p).map(|c| axis_range(data, c));

    let stride = (n / spec.max_points.max(1)).max(1);
    let mut points = Vec::with_capacity((n / stride).max(1));
    for row in (0..n).step_by(stride) {
        let x = norm01(data[[row, spec.x]] as f64, xr) * 2.0 - 1.0;
        let y = norm01(data[[row, spec.y]] as f64, yr) * 2.0 - 1.0;
        let z01 = norm01(data[[row, spec.z]] as f64, zr);
        let z = z01 * 2.0 - 1.0;
        let t = match (spec.color, cr) {
            (Some(c), Some(range)) if c < p => norm01(data[[row, c]] as f64, range),
            _ => z01,
        };
        let colour = colour_u8(density_colour(&spec.density_map, t));
        let size = match (spec.size, sr) {
            (Some(s), Some(range)) if s < p => 0.5 + norm01(data[[row, s]] as f64, range) * 2.0,
            _ => 1.0,
        };
        points.push(Point3 {
            pos: [x, y, z],
            color: colour,
            size,
        });
    }
    points
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn header(width: u32, height: u32, background: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" style="background:{background}">"#
    )
}

fn axes_svg(camera: &Camera, width: u32, height: u32) -> String {
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
    let projected: Vec<[f64; 2]> = corners
        .iter()
        .map(|&c| project_point(c, camera, width, height).screen)
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
    let mut svg = String::new();
    for (a, b) in edges {
        let pa = projected[a];
        let pb = projected[b];
        svg.push_str(&format!(
            "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"#94a3b8\" stroke-width=\"0.8\" stroke-opacity=\"0.55\" />\n",
            pa[0], pa[1], pb[0], pb[1]
        ));
    }
    // Axis labels at three positive corners.
    for (idx, label) in [(1, "X"), (3, "Y"), (5, "Z")] {
        let p = projected[idx];
        svg.push_str(&format!(
            "<text x=\"{:.2}\" y=\"{:.2}\" font-size=\"11\" fill=\"#64748b\" text-anchor=\"middle\">{}</text>\n",
            p[0], p[1] - 6.0, label
        ));
    }
    svg
}

/// Renders normalized points (plus a cube frame) as SVG.
pub fn render_points_svg(
    points: &[Point3],
    camera: &Camera,
    width: u32,
    height: u32,
    background: &str,
) -> String {
    let mut svg = header(width, height, background);
    svg.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"{}\" />\n",
        escape(background)
    ));
    svg.push_str(&axes_svg(camera, width, height));

    let mut sorted: Vec<(Projected, &Point3)> = points
        .iter()
        .map(|p| (project_point(p.pos, camera, width, height), p))
        .collect();
    sorted.sort_by(|a, b| {
        a.0.depth
            .partial_cmp(&b.0.depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let base_radius = 1.8 * camera.zoom.sqrt();
    for (projected, point) in sorted {
        let alpha = (0.35 + 0.65 * ((projected.depth + 1.5) / 3.0).clamp(0.0, 1.0)).min(1.0);
        let (r, g, b) = point.color;
        svg.push_str(&format!(
            "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" fill=\"#{:02x}{:02x}{:02x}\" fill-opacity=\"{alpha:.2}\" />\n",
            projected.screen[0],
            projected.screen[1],
            (base_radius * point.size).clamp(0.4, 18.0),
            r,
            g,
            b
        ));
    }
    svg.push_str("</svg>\n");
    svg
}

/// A projected shape used by the GUI painter (and reused by SVG export).
#[derive(Debug, Clone)]
pub enum PreviewShape {
    /// A filled circle.
    Dot {
        center: [f64; 2],
        radius: f64,
        color: (u8, u8, u8),
        opacity: f64,
    },
    /// A filled polygon.
    Poly {
        points: Vec<[f64; 2]>,
        color: (u8, u8, u8),
    },
}

fn terrain_shapes(
    data: &ndarray::Array2<f32>,
    spec: &Spec3D,
    width: u32,
    height: u32,
) -> Vec<PreviewShape> {
    let (n, p) = data.dim();
    if n == 0 || spec.x >= p || spec.y >= p {
        return Vec::new();
    }
    let xr = axis_range(data, spec.x);
    let yr = axis_range(data, spec.y);
    let grid = spec.terrain_grid.max(2);
    let mut counts = vec![0f64; grid * grid];
    for row in 0..n {
        let x = norm01(data[[row, spec.x]] as f64, xr);
        let y = norm01(data[[row, spec.y]] as f64, yr);
        let ix = ((x * (grid - 1) as f64).round() as usize).min(grid - 1);
        let iy = ((y * (grid - 1) as f64).round() as usize).min(grid - 1);
        counts[iy * grid + ix] += 1.0;
    }
    let max = counts.iter().copied().fold(0.0, f64::max).max(1.0);
    let cell = 2.0 / grid as f64;
    let mut quads: Vec<(f64, PreviewShape)> = Vec::new();
    for iy in 0..grid {
        for ix in 0..grid {
            let count = counts[iy * grid + ix];
            if count <= 0.0 {
                continue;
            }
            let t = count / max;
            let cx = -1.0 + (ix as f64 + 0.5) * cell;
            let cy = -1.0 + (iy as f64 + 0.5) * cell;
            let z = t * 1.8 - 0.9;
            let corners = [
                [cx - cell / 2.0, cy - cell / 2.0, z],
                [cx + cell / 2.0, cy - cell / 2.0, z],
                [cx + cell / 2.0, cy + cell / 2.0, z],
                [cx - cell / 2.0, cy + cell / 2.0, z],
            ];
            let projected: Vec<Projected> = corners
                .iter()
                .map(|&c| project_point(c, &spec.camera, width, height))
                .collect();
            let depth = projected.iter().map(|p| p.depth).sum::<f64>() / 4.0;
            quads.push((
                depth,
                PreviewShape::Poly {
                    points: projected.iter().map(|p| p.screen).collect(),
                    color: colour_u8(density_colour(&spec.density_map, t)),
                },
            ));
        }
    }
    quads.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    quads.into_iter().map(|(_, shape)| shape).collect()
}

fn voxel_shapes(
    data: &ndarray::Array2<f32>,
    spec: &Spec3D,
    width: u32,
    height: u32,
) -> Vec<PreviewShape> {
    let (n, p) = data.dim();
    if n == 0 || spec.x >= p || spec.y >= p || spec.z >= p {
        return Vec::new();
    }
    let xr = axis_range(data, spec.x);
    let yr = axis_range(data, spec.y);
    let zr = axis_range(data, spec.z);
    let grid = spec.voxel_grid.max(2);
    let mut counts = vec![0f64; grid * grid * grid];
    for row in 0..n {
        let x = norm01(data[[row, spec.x]] as f64, xr);
        let y = norm01(data[[row, spec.y]] as f64, yr);
        let z = norm01(data[[row, spec.z]] as f64, zr);
        let ix = ((x * (grid - 1) as f64).round() as usize).min(grid - 1);
        let iy = ((y * (grid - 1) as f64).round() as usize).min(grid - 1);
        let iz = ((z * (grid - 1) as f64).round() as usize).min(grid - 1);
        counts[(iz * grid + iy) * grid + ix] += 1.0;
    }
    let max = counts.iter().copied().fold(0.0, f64::max).max(1.0);
    let cell = 2.0 / grid as f64;
    let mut squares: Vec<(f64, PreviewShape)> = Vec::new();
    for iz in 0..grid {
        for iy in 0..grid {
            for ix in 0..grid {
                let count = counts[(iz * grid + iy) * grid + ix];
                if count <= 0.0 {
                    continue;
                }
                let t = count / max;
                let cx = -1.0 + (ix as f64 + 0.5) * cell;
                let cy = -1.0 + (iy as f64 + 0.5) * cell;
                let cz = -1.0 + (iz as f64 + 0.5) * cell;
                let projected = project_point([cx, cy, cz], &spec.camera, width, height);
                let side = (width.min(height) as f64 * 0.4 * spec.camera.zoom * cell * 0.55)
                    .clamp(0.6, 22.0);
                let corners = [
                    projected.screen[0] - side / 2.0,
                    projected.screen[1] - side / 2.0,
                    projected.screen[0] + side / 2.0,
                    projected.screen[1] + side / 2.0,
                ];
                squares.push((
                    projected.depth,
                    PreviewShape::Poly {
                        points: vec![
                            [corners[0], corners[1]],
                            [corners[2], corners[1]],
                            [corners[2], corners[3]],
                            [corners[0], corners[3]],
                        ],
                        color: colour_u8(density_colour(&spec.density_map, t)),
                    },
                ));
            }
        }
    }
    squares.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    squares.into_iter().map(|(_, shape)| shape).collect()
}

/// Projects the current 3D view into drawable shapes for the GUI painter.
pub fn preview_shapes(
    data: &ndarray::Array2<f32>,
    spec: &Spec3D,
    width: u32,
    height: u32,
) -> Vec<PreviewShape> {
    match spec.mode {
        View3DMode::PointCloud | View3DMode::Bubble => {
            let points = build_points(data, spec);
            let base_radius = 1.8 * spec.camera.zoom.sqrt();
            let mut sorted: Vec<(Projected, &Point3)> = points
                .iter()
                .map(|p| (project_point(p.pos, &spec.camera, width, height), p))
                .collect();
            sorted.sort_by(|a, b| {
                a.0.depth
                    .partial_cmp(&b.0.depth)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            sorted
                .into_iter()
                .map(|(projected, point)| PreviewShape::Dot {
                    center: projected.screen,
                    radius: (base_radius * point.size).clamp(0.4, 18.0),
                    color: point.color,
                    opacity: (0.35 + 0.65 * ((projected.depth + 1.5) / 3.0).clamp(0.0, 1.0))
                        .min(1.0),
                })
                .collect()
        }
        View3DMode::Terrain => terrain_shapes(data, spec, width, height),
        View3DMode::Voxel => voxel_shapes(data, spec, width, height),
    }
}

/// Renders a terrain surface from a 2D density grid over X/Y.
pub fn terrain_svg(
    data: &ndarray::Array2<f32>,
    spec: &Spec3D,
    width: u32,
    height: u32,
    background: &str,
) -> String {
    let mut svg = header(width, height, background);
    svg.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"{}\" />\n",
        escape(background)
    ));
    svg.push_str(&axes_svg(&spec.camera, width, height));
    for shape in terrain_shapes(data, spec, width, height) {
        if let PreviewShape::Poly { points, color } = shape {
            let (r, g, b) = color;
            let mut path = format!("M {:.2} {:.2}", points[0][0], points[0][1]);
            for p in &points[1..] {
                path.push_str(&format!(" L {:.2} {:.2}", p[0], p[1]));
            }
            path.push_str(" Z");
            svg.push_str(&format!(
                "<path d=\"{path}\" fill=\"#{r:02x}{g:02x}{b:02x}\" fill-opacity=\"0.85\" stroke=\"#{r:02x}{g:02x}{b:02x}\" stroke-width=\"0.4\" />\n"
            ));
        }
    }
    svg.push_str("</svg>\n");
    svg
}

/// Renders occupied 3D voxels as depth-sorted squares.
pub fn voxel_svg(
    data: &ndarray::Array2<f32>,
    spec: &Spec3D,
    width: u32,
    height: u32,
    background: &str,
) -> String {
    let mut svg = header(width, height, background);
    svg.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"{}\" />\n",
        escape(background)
    ));
    svg.push_str(&axes_svg(&spec.camera, width, height));
    for shape in voxel_shapes(data, spec, width, height) {
        if let PreviewShape::Poly { points, color } = shape {
            let (r, g, b) = color;
            let x0 = points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
            let x1 = points
                .iter()
                .map(|p| p[0])
                .fold(f64::NEG_INFINITY, f64::max);
            let y0 = points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
            let y1 = points
                .iter()
                .map(|p| p[1])
                .fold(f64::NEG_INFINITY, f64::max);
            svg.push_str(&format!(
                "<rect x=\"{x0:.2}\" y=\"{y0:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"#{r:02x}{g:02x}{b:02x}\" fill-opacity=\"0.8\" stroke=\"#{r:02x}{g:02x}{b:02x}\" stroke-width=\"0.4\" />\n",
                x1 - x0,
                y1 - y0
            ));
        }
    }
    svg.push_str("</svg>\n");
    svg
}

/// Renders any 3D view mode to SVG.
pub fn render_3d(
    data: &ndarray::Array2<f32>,
    spec: &Spec3D,
    width: u32,
    height: u32,
    background: &str,
) -> String {
    match spec.mode {
        View3DMode::PointCloud | View3DMode::Bubble => {
            let points = build_points(data, spec);
            render_points_svg(&points, &spec.camera, width, height, background)
        }
        View3DMode::Terrain => terrain_svg(data, spec, width, height, background),
        View3DMode::Voxel => voxel_svg(data, spec, width, height, background),
    }
}

/// Renders a rotation animation as one SVG frame per `degrees_per_frame`.
///
/// Exporting a numbered frame sequence keeps the crate dependency-free while
/// still producing a deterministic animation that any image tool can assemble.
pub fn rotation_frames_svg(
    data: &ndarray::Array2<f32>,
    spec: &Spec3D,
    frames: usize,
    degrees_per_frame: f64,
    width: u32,
    height: u32,
    background: &str,
) -> Vec<String> {
    (0..frames.max(1))
        .map(|i| {
            let mut frame = spec.clone();
            frame.camera.yaw_deg = spec.camera.yaw_deg + i as f64 * degrees_per_frame;
            render_3d(data, &frame, width, height, background)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    fn sample() -> ndarray::Array2<f32> {
        array![
            [1.0, 2.0, 3.0, 10.0],
            [2.0, 4.0, 4.0, 20.0],
            [3.0, 6.0, 5.0, 30.0],
            [4.0, 8.0, 6.0, 40.0],
            [5.0, 10.0, 7.0, 50.0]
        ]
    }

    #[test]
    fn camera_rotates_and_projects_centre() {
        let camera = Camera::default();
        let p = project_point([0.0, 0.0, 0.0], &camera, 400, 300);
        assert!((p.screen[0] - 200.0).abs() < 1e-9);
        assert!((p.screen[1] - 150.0).abs() < 1e-9);
        // A yaw rotation moves an off-axis point.
        let rotated = camera.rotate([1.0, 0.0, 0.0]);
        assert!(rotated[0] < 1.0);
        assert!(rotated[2].abs() > 0.0);
    }

    #[test]
    fn build_points_normalizes_axes_and_maps_colour_and_size() {
        let data = sample();
        let spec = Spec3D {
            x: 0,
            y: 1,
            z: 2,
            color: Some(3),
            size: Some(0),
            ..Default::default()
        };
        let points = build_points(&data, &spec);
        assert_eq!(points.len(), 5);
        assert!((points[0].pos[0] + 1.0).abs() < 1e-9);
        assert!((points[4].pos[0] - 1.0).abs() < 1e-9);
        assert!(points[4].size > points[0].size);
        assert_ne!(points[0].color, points[4].color);
    }

    #[test]
    fn point_cloud_svg_contains_circles_and_axes() {
        let data = sample();
        let spec = Spec3D::default();
        let svg = render_3d(&data, &spec, 400, 300, "#ffffff");
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<circle"));
        assert!(svg.contains(">X<"));
        assert!(svg.ends_with("</svg>\n"));
    }

    #[test]
    fn terrain_and_voxel_modes_render_shapes() {
        let data = sample();
        let terrain = render_3d(
            &data,
            &Spec3D {
                mode: View3DMode::Terrain,
                ..Default::default()
            },
            320,
            320,
            "#ffffff",
        );
        assert!(terrain.contains("<path"));

        let voxel = render_3d(
            &data,
            &Spec3D {
                mode: View3DMode::Voxel,
                voxel_grid: 4,
                ..Default::default()
            },
            320,
            320,
            "#ffffff",
        );
        assert!(voxel.contains("<rect"));
    }

    #[test]
    fn rotation_frames_are_deterministic_and_differ() {
        let data = sample();
        let spec = Spec3D::default();
        let frames = rotation_frames_svg(&data, &spec, 4, 15.0, 300, 300, "#ffffff");
        assert_eq!(frames.len(), 4);
        assert_ne!(frames[0], frames[1]);
        // Rendering the same camera twice is deterministic.
        let again = render_3d(&data, &spec, 300, 300, "#ffffff");
        assert_eq!(frames[0], again);
    }

    #[test]
    fn mode_ids_round_trip() {
        for mode in View3DMode::all() {
            assert_eq!(View3DMode::from_id(mode.id()), Some(mode));
        }
        assert_eq!(View3DMode::from_id("bogus"), None);
    }
}
