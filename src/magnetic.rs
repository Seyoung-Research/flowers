//! Magnetic gates and density snapping.
//!
//! While drawing a gate, FlowJo can pull the pointer toward dense parts of the
//! plot so a boundary lands on the population instead of in empty space. This
//! module provides the pure geometry behind that interaction, independent of
//! egui:
//!
//! * [`DensityField`] — a regular grid of event counts built from the plotted
//!   points, with bilinear sampling.
//! * [`SnapSettings`] — on/off switch, attraction radius and strength.
//! * [`snap_to_density`] — local gradient ascent toward a density maximum.
//! * [`snap_rectangle`] / [`snap_polygon`] — snap the corners/vertices of a
//!   gate while it is drawn.
//!
//! All helpers are deterministic and leave the input point unchanged when
//! snapping is disabled or no density gradient is available nearby.

use crate::plot::{DensityGrid, density_grid};

/// Settings for density snapping while drawing.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapSettings {
    /// Whether snapping is active.
    pub enabled: bool,
    /// Search radius in data units. Points farther than this from any density
    /// maximum are left untouched.
    pub radius: f64,
    /// Attraction strength in `[0, 1]`: 0 leaves the point in place, 1 moves it
    /// all the way to the snapped position.
    pub strength: f64,
    /// Maximum number of gradient-ascent steps.
    pub max_iterations: usize,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            radius: 12.0,
            strength: 0.6,
            max_iterations: 24,
        }
    }
}

impl SnapSettings {
    /// An enabled configuration with the given radius and strength.
    pub fn enabled(radius: f64, strength: f64) -> Self {
        Self {
            enabled: true,
            radius,
            strength,
            ..Self::default()
        }
    }

    /// Clamp the strength into `[0, 1]` and the radius to a non-negative value.
    pub fn sanitised(&self) -> Self {
        Self {
            enabled: self.enabled,
            radius: self.radius.max(0.0),
            strength: self.strength.clamp(0.0, 1.0),
            max_iterations: self.max_iterations.max(1),
        }
    }
}

/// A density field over the plot area, used for magnetic snapping.
#[derive(Debug, Clone)]
pub struct DensityField {
    grid: DensityGrid,
}

impl DensityField {
    /// Build a field from plotted points over the given data-space rectangle.
    ///
    /// `nx`/`ny` are clamped to at least 2 cells. Non-finite points are
    /// ignored by [`density_grid`].
    pub fn from_points(
        points: &[[f64; 2]],
        x0: f64,
        x1: f64,
        y0: f64,
        y1: f64,
        nx: usize,
        ny: usize,
    ) -> Self {
        Self {
            grid: density_grid(points, x0, x1, y0, y1, nx.max(2), ny.max(2)),
        }
    }

    /// The underlying density grid.
    pub fn grid(&self) -> &DensityGrid {
        &self.grid
    }

    /// Bilinearly interpolated density at `point`.
    ///
    /// Points outside the grid sample the nearest edge, matching the plot's
    /// density colouring.
    pub fn density_at(&self, point: [f64; 2]) -> f64 {
        crate::plot::point_densities(&[point], &self.grid)[0]
    }

    /// Highest density anywhere in the field.
    pub fn max_density(&self) -> f64 {
        self.grid.max()
    }

    /// Data-space size of one grid cell (used as the gradient-ascent step).
    fn cell_size(&self) -> (f64, f64) {
        let g = &self.grid;
        let dx = if g.nx > 1 {
            (g.x1 - g.x0).abs() / (g.nx - 1) as f64
        } else {
            1.0
        };
        let dy = if g.ny > 1 {
            (g.y1 - g.y0).abs() / (g.ny - 1) as f64
        } else {
            1.0
        };
        (dx.max(f64::EPSILON), dy.max(f64::EPSILON))
    }

    /// Central-difference density gradient at `point`.
    fn gradient_at(&self, point: [f64; 2]) -> [f64; 2] {
        let (dx, dy) = self.cell_size();
        let half = 0.5;
        let gx = (self.density_at([point[0] + dx * half, point[1]])
            - self.density_at([point[0] - dx * half, point[1]]))
            / dx;
        let gy = (self.density_at([point[0], point[1] + dy * half])
            - self.density_at([point[0], point[1] - dy * half]))
            / dy;
        [gx, gy]
    }
}

/// Move `point` toward a nearby density maximum.
///
/// Attraction is computed in two stages: first the density-weighted centroid of
/// all occupied cells within `radius` gives a robust direction even when the
/// histogram is sparse, then local gradient ascent refines the target. The
/// search is bounded by `radius` in data units, so a gate corner is never
/// pulled across the plot. The returned point blends the original position with
/// the snapped position by `settings.strength` (0 = unchanged, 1 = fully
/// snapped).
///
/// When snapping is disabled, the point lies outside the field, or no occupied
/// cells are within the radius, `point` is returned unchanged.
pub fn snap_to_density(point: [f64; 2], field: &DensityField, settings: &SnapSettings) -> [f64; 2] {
    let settings = settings.sanitised();
    if !settings.enabled || settings.radius <= 0.0 || settings.strength <= 0.0 {
        return point;
    }
    let grid = field.grid();
    let inside = point[0] >= grid.x0.min(grid.x1)
        && point[0] <= grid.x0.max(grid.x1)
        && point[1] >= grid.y0.min(grid.y1)
        && point[1] <= grid.y0.max(grid.y1);
    if !inside {
        return point;
    }

    // Stage 1: density-weighted centroid of occupied cells inside the radius.
    let mut sum = [0.0f64, 0.0f64];
    let mut total = 0.0f64;
    for j in 0..grid.ny {
        let y = if grid.ny <= 1 {
            grid.y0
        } else {
            grid.y0 + (grid.y1 - grid.y0) * j as f64 / (grid.ny - 1) as f64
        };
        for i in 0..grid.nx {
            let value = grid.at(i, j);
            if value <= 0.0 {
                continue;
            }
            let x = if grid.nx <= 1 {
                grid.x0
            } else {
                grid.x0 + (grid.x1 - grid.x0) * i as f64 / (grid.nx - 1) as f64
            };
            let dx = x - point[0];
            let dy = y - point[1];
            if dx.hypot(dy) > settings.radius {
                continue;
            }
            sum[0] += x * value;
            sum[1] += y * value;
            total += value;
        }
    }
    if total <= 0.0 {
        return point;
    }
    let centroid = [sum[0] / total, sum[1] / total];

    // Stage 2: gradient ascent from the centroid, kept inside the radius.
    let mut current = centroid;
    let mut best = centroid;
    let mut best_density = field.density_at(centroid);
    let (dx, dy) = field.cell_size();
    for _ in 0..settings.max_iterations {
        let [gx, gy] = field.gradient_at(current);
        let norm = gx.hypot(gy);
        if norm <= 1e-12 {
            break;
        }
        let step = 0.5;
        let mut next = [
            current[0] + step * dx * gx / norm,
            current[1] + step * dy * gy / norm,
        ];
        let off_x = next[0] - point[0];
        let off_y = next[1] - point[1];
        let dist = off_x.hypot(off_y);
        if dist > settings.radius {
            let scale = settings.radius / dist;
            next = [point[0] + off_x * scale, point[1] + off_y * scale];
        }
        let density = field.density_at(next);
        if density > best_density {
            best = next;
            best_density = density;
        }
        if (next[0] - current[0]).abs() <= 1e-12 && (next[1] - current[1]).abs() <= 1e-12 {
            break;
        }
        current = next;
    }

    // The centroid itself can lie slightly beyond the radius when the field is
    // asymmetric; clamp the final displacement.
    let mut delta = [best[0] - point[0], best[1] - point[1]];
    let dist = delta[0].hypot(delta[1]);
    if dist > settings.radius {
        let scale = settings.radius / dist;
        delta = [delta[0] * scale, delta[1] * scale];
    }
    [
        point[0] + delta[0] * settings.strength,
        point[1] + delta[1] * settings.strength,
    ]
}

/// Snap both corners of a rectangle being drawn.
///
/// Returns `(min_x, max_x, min_y, max_y)` in sorted order, so the result is
/// always a valid rectangle even when the two snapped corners cross.
pub fn snap_rectangle(
    start: [f64; 2],
    end: [f64; 2],
    field: &DensityField,
    settings: &SnapSettings,
) -> (f64, f64, f64, f64) {
    let a = snap_to_density(start, field, settings);
    let b = snap_to_density(end, field, settings);
    (
        a[0].min(b[0]),
        a[0].max(b[0]),
        a[1].min(b[1]),
        a[1].max(b[1]),
    )
}

/// Snap every vertex of a polygon being drawn.
///
/// A vertex that snaps onto a duplicate of its neighbour is dropped, so a
/// degenerate edge cannot be introduced. At least three vertices are preserved
/// when the input had at least three.
pub fn snap_polygon(
    vertices: &[[f64; 2]],
    field: &DensityField,
    settings: &SnapSettings,
) -> Vec<[f64; 2]> {
    let settings = settings.sanitised();
    if !settings.enabled || settings.radius <= 0.0 || settings.strength <= 0.0 {
        return vertices.to_vec();
    }
    fn dedupe(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
        let mut out: Vec<[f64; 2]> = Vec::with_capacity(points.len());
        for &point in points {
            let duplicate = out.last().is_some_and(|last| {
                (last[0] - point[0]).abs() <= f64::EPSILON
                    && (last[1] - point[1]).abs() <= f64::EPSILON
            });
            if !duplicate {
                out.push(point);
            }
        }
        // A closed polygon must not end with a repeated first vertex.
        if out.len() > 1
            && (out[0][0] - out[out.len() - 1][0]).abs() <= f64::EPSILON
            && (out[0][1] - out[out.len() - 1][1]).abs() <= f64::EPSILON
        {
            out.pop();
        }
        out
    }

    let snapped: Vec<[f64; 2]> = vertices
        .iter()
        .map(|&vertex| snap_to_density(vertex, field, &settings))
        .collect();
    let out = dedupe(&snapped);
    // Snapping must not reduce a polygon below a drawable vertex count; fall
    // back to the user's original vertices (minus exact duplicates) if it did.
    if vertices.len() >= 3 && out.len() < 3 {
        return dedupe(vertices);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_blobs() -> Vec<[f64; 2]> {
        // Two solid clusters near (2, 2) and (18, 18), plus sparse noise.
        let mut points = Vec::new();
        for i in 0..60 {
            let angle = i as f64 * 0.7;
            points.push([2.0 + 0.15 * angle.cos(), 2.0 + 0.15 * angle.sin()]);
            points.push([18.0 + 0.15 * angle.cos(), 18.0 + 0.15 * angle.sin()]);
        }
        points.push([10.0, 10.0]);
        points
    }

    fn field() -> DensityField {
        DensityField::from_points(&two_blobs(), 0.0, 20.0, 0.0, 20.0, 40, 40)
    }

    #[test]
    fn density_field_samples_clusters() {
        let field = field();
        let near_cluster = field.density_at([2.0, 2.0]);
        let empty = field.density_at([12.0, 4.0]);
        assert!(near_cluster > empty);
        assert!(field.max_density() > 0.0);
    }

    #[test]
    fn snap_pulls_point_toward_dense_cluster() {
        let field = field();
        let settings = SnapSettings::enabled(6.0, 1.0);
        let snapped = snap_to_density([4.5, 4.5], &field, &settings);
        assert!(snapped[0] < 4.5 && snapped[1] < 4.5, "snapped: {snapped:?}");
        let before = field.density_at([4.5, 4.5]);
        let after = field.density_at(snapped);
        assert!(after >= before, "before={before} after={after}");
    }

    #[test]
    fn disabled_or_flat_settings_leave_point_unchanged() {
        let field = field();
        let point = [4.5, 4.5];
        assert_eq!(
            snap_to_density(point, &field, &SnapSettings::default()),
            point
        );
        assert_eq!(
            snap_to_density(point, &field, &SnapSettings::enabled(6.0, 0.0)),
            point
        );
        // Outside the density grid nothing moves.
        assert_eq!(
            snap_to_density([-50.0, -50.0], &field, &SnapSettings::enabled(6.0, 1.0)),
            [-50.0, -50.0]
        );
    }

    #[test]
    fn strength_blends_between_original_and_snapped() {
        let field = field();
        let point = [4.5, 4.5];
        let full = snap_to_density(point, &field, &SnapSettings::enabled(8.0, 1.0));
        let half = snap_to_density(point, &field, &SnapSettings::enabled(8.0, 0.5));
        let expected = [
            point[0] + (full[0] - point[0]) * 0.5,
            point[1] + (full[1] - point[1]) * 0.5,
        ];
        assert!((half[0] - expected[0]).abs() < 1e-9);
        assert!((half[1] - expected[1]).abs() < 1e-9);
    }

    #[test]
    fn radius_limits_the_attraction_distance() {
        let field = field();
        let point = [7.5, 7.5];
        let settings = SnapSettings::enabled(1.0, 1.0);
        let snapped = snap_to_density(point, &field, &settings);
        let distance = ((snapped[0] - point[0]).powi(2) + (snapped[1] - point[1]).powi(2)).sqrt();
        assert!(distance <= 1.0 + 1e-9, "distance={distance}");
    }

    #[test]
    fn rectangle_snapping_sorts_corners() {
        let field = field();
        let settings = SnapSettings::enabled(6.0, 1.0);
        let (min_x, max_x, min_y, max_y) =
            snap_rectangle([5.0, 5.0], [1.0, 1.0], &field, &settings);
        assert!(min_x <= max_x);
        assert!(min_y <= max_y);
        // Both corners are attracted toward the dense lower-left cluster.
        assert!(min_x < 5.0 && max_x <= 5.0);
    }

    #[test]
    fn polygon_snapping_drops_duplicate_vertices_but_stays_drawable() {
        let field = field();
        let settings = SnapSettings::enabled(6.0, 1.0);
        let vertices = vec![[4.5, 4.5], [4.5, 4.5], [5.0, 5.0], [1.5, 1.5]];
        let snapped = snap_polygon(&vertices, &field, &settings);
        assert!(snapped.len() >= 3);
        for pair in snapped.windows(2) {
            assert!(
                (pair[0][0] - pair[1][0]).abs() > f64::EPSILON
                    || (pair[0][1] - pair[1][1]).abs() > f64::EPSILON
            );
        }

        // With snapping disabled the vertices are returned untouched.
        assert_eq!(
            snap_polygon(&vertices, &field, &SnapSettings::default()),
            vertices
        );
    }
}
