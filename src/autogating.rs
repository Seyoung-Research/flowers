//! Automatic gating helpers.
//!
//! These algorithms generate gates from the data instead of asking the user to
//! draw them:
//!
//! * [`polygon_autogate`] — convex hull around a selected population.
//! * [`find_peaks`] / [`gate_on_peak`] — histogram peaks and ranges around one
//!   peak.
//! * [`bifurcate`] — Otsu/BifurGate threshold separating a bimodal parameter
//!   into low and high populations.
//! * [`kmeans_clusters`] / [`cluster_gates`] — deterministic k-means on two
//!   parameters with one polygon gate per cluster.
//!
//! All algorithms are deterministic: repeated runs on the same input produce
//! the same gates.

use ndarray::Array2;

use crate::gating::{PolygonGate, RangeGate};

/// Collect finite `(x, y)` pairs for the selected event indices.
fn finite_points(
    data: &Array2<f32>,
    indices: &[usize],
    x_idx: usize,
    y_idx: usize,
) -> Vec<(usize, f32, f32)> {
    indices
        .iter()
        .filter_map(|&i| {
            let x = *data.get((i, x_idx))?;
            let y = *data.get((i, y_idx))?;
            (x.is_finite() && y.is_finite()).then_some((i, x, y))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Polygon autogate (convex hull)
// ---------------------------------------------------------------------------

/// Andrew's monotone-chain convex hull, returned counter-clockwise.
///
/// Collinear points are omitted; fewer than three input points are returned
/// as-is (after sorting and de-duplication).
pub fn convex_hull(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    pts.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    if pts.len() <= 2 {
        return pts;
    }

    fn cross(o: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    }

    let mut lower: Vec<(f32, f32)> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(f32, f32)> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Generate a polygon gate tightly enclosing the selected events.
///
/// The gate is the convex hull of the population's `(x, y)` points, expanded
/// about its centroid by `padding` (a fraction; `0.05` grows it by 5%). Returns
/// `None` when fewer than three distinct finite points are available, because
/// a polygon gate needs at least three vertices.
pub fn polygon_autogate(
    data: &Array2<f32>,
    indices: &[usize],
    x_idx: usize,
    y_idx: usize,
    padding: f32,
) -> Option<PolygonGate> {
    let points: Vec<(f32, f32)> = finite_points(data, indices, x_idx, y_idx)
        .into_iter()
        .map(|(_, x, y)| (x, y))
        .collect();
    let hull = convex_hull(&points);
    if hull.len() < 3 {
        return None;
    }
    let (cx, cy) = hull.iter().fold((0.0f64, 0.0f64), |(sx, sy), &(x, y)| {
        (sx + x as f64, sy + y as f64)
    });
    let cx = (cx / hull.len() as f64) as f32;
    let cy = (cy / hull.len() as f64) as f32;
    let scale = 1.0 + padding.max(0.0);
    let vertices: Vec<(f32, f32)> = hull
        .iter()
        .map(|&(x, y)| (cx + (x - cx) * scale, cy + (y - cy) * scale))
        .collect();
    Some(PolygonGate::new(x_idx, y_idx, vertices))
}

// ---------------------------------------------------------------------------
// Gate on peaks (1D histogram)
// ---------------------------------------------------------------------------

/// One histogram peak and the range around it.
#[derive(Debug, Clone, PartialEq)]
pub struct Peak {
    /// Peak centre in data units.
    pub center: f64,
    /// Smoothed histogram height at the peak.
    pub height: f64,
    /// Lower gate boundary (the valley to the left, or the data minimum).
    pub start: f64,
    /// Upper gate boundary (the valley to the right, or the data maximum).
    pub end: f64,
    /// Number of events in the peak's histogram bin.
    pub count: usize,
}

impl Peak {
    /// A range gate around this peak on `param_idx`.
    pub fn gate(&self, param_idx: usize) -> RangeGate {
        RangeGate::new(param_idx, self.start as f32, self.end as f32)
    }
}

fn histogram(values: &[f64], bins: usize) -> Option<(Vec<f64>, f64, f64)> {
    let mut finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return None;
    }
    finite.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min = finite[0];
    let max = finite[finite.len() - 1];
    let bins = bins.max(2);
    if (max - min).abs() < 1e-12 {
        return Some((vec![finite.len() as f64], min, 1.0));
    }
    let width = (max - min) / bins as f64;
    let mut counts = vec![0.0f64; bins];
    for v in finite {
        let idx = (((v - min) / width).floor() as usize).min(bins - 1);
        counts[idx] += 1.0;
    }
    Some((counts, min, width))
}

fn smoothed(counts: &[f64]) -> Vec<f64> {
    counts
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let lo = i.saturating_sub(2);
            let hi = (i + 2).min(counts.len() - 1);
            counts[lo..=hi].iter().sum::<f64>() / (hi - lo + 1) as f64
        })
        .collect()
}

/// Find peaks in a 1D parameter histogram.
///
/// `bins` controls histogram resolution (at least 2). Peaks are local maxima of
/// a 5-bin moving average, with a default prominence filter of 10% of the
/// tallest peak (pass a lower fraction to keep shoulders). Each peak's `start`/`end` extend to the valleys on either
/// side, making it usable directly as a [`RangeGate`].
///
/// Results are sorted by peak centre (ascending), which keeps peak indices
/// stable for a given dataset.
pub fn find_peaks(data: &Array2<f32>, param_idx: usize, bins: usize) -> Vec<Peak> {
    find_peaks_with_prominence(data, param_idx, bins, 0.1)
}

/// Like [`find_peaks`] with an explicit minimum prominence as a fraction of
/// the tallest peak (e.g. `0.05` keeps peaks at least 5% as tall).
pub fn find_peaks_with_prominence(
    data: &Array2<f32>,
    param_idx: usize,
    bins: usize,
    min_prominence: f64,
) -> Vec<Peak> {
    let values: Vec<f64> = (0..data.nrows())
        .filter_map(|i| data.get((i, param_idx)).copied())
        .filter(|v| v.is_finite())
        .map(|v| v as f64)
        .collect();
    let Some((counts, min, width)) = histogram(&values, bins.max(2)) else {
        return Vec::new();
    };
    if counts.len() == 1 {
        let value = min;
        return vec![Peak {
            center: value,
            height: counts[0],
            start: value,
            end: value,
            count: counts[0] as usize,
        }];
    }
    let smooth = smoothed(&counts);
    let max_height = smooth.iter().copied().fold(0.0f64, f64::max);
    if max_height <= 0.0 {
        return Vec::new();
    }
    let min_prominence = min_prominence.clamp(0.0, 1.0);

    // Local maxima of the smoothed histogram, with plateau de-duplication.
    let mut peak_bins: Vec<usize> = Vec::new();
    let mut i = 1;
    while i + 1 < smooth.len() {
        if smooth[i] >= smooth[i - 1] && smooth[i] >= smooth[i + 1] {
            peak_bins.push(i);
            let height = smooth[i];
            while i + 1 < smooth.len() && (smooth[i + 1] - height).abs() < 1e-12 {
                i += 1;
            }
        }
        i += 1;
    }
    if peak_bins.is_empty() {
        return Vec::new();
    }

    // Topographic prominence: how far a peak rises above the higher of the
    // valleys flanking it. This suppresses histogram-noise shoulders while
    // keeping genuine modes.
    peak_bins.retain(|&p| {
        let mut left = p;
        while left > 0 && smooth[left - 1] <= smooth[left] {
            left -= 1;
        }
        let mut right = p;
        while right + 1 < smooth.len() && smooth[right + 1] <= smooth[right] {
            right += 1;
        }
        let left_valley = if left == 0 { 0.0 } else { smooth[left] };
        let right_valley = if right + 1 >= smooth.len() {
            0.0
        } else {
            smooth[right]
        };
        smooth[p] - left_valley.max(right_valley) >= max_height * min_prominence
    });
    if peak_bins.is_empty() {
        return Vec::new();
    }

    // Boundaries are the valleys between adjacent peaks (or the data limits
    // for the outermost peaks), so the resulting range gates tile the data.
    let mut peaks = Vec::with_capacity(peak_bins.len());
    for (idx, &p) in peak_bins.iter().enumerate() {
        let start = if idx == 0 {
            min
        } else {
            let previous = peak_bins[idx - 1];
            let valley = (previous + 1..p)
                .min_by(|&a, &b| {
                    smooth[a]
                        .partial_cmp(&smooth[b])
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(previous);
            min + (valley as f64 + 0.5) * width
        };
        let end = if idx + 1 == peak_bins.len() {
            min + counts.len() as f64 * width
        } else {
            let next = peak_bins[idx + 1];
            let valley = (p + 1..next)
                .min_by(|&a, &b| {
                    smooth[a]
                        .partial_cmp(&smooth[b])
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(next);
            min + (valley as f64 + 0.5) * width
        };
        peaks.push(Peak {
            center: min + (p as f64 + 0.5) * width,
            height: smooth[p],
            start,
            end,
            count: counts[p] as usize,
        });
    }
    peaks
}

/// Build a range gate around one peak from [`find_peaks`].
///
/// `peak_index` is the position in the ascending-centre peak list. Returns
/// `None` when the parameter has no peaks or the index is out of range.
pub fn gate_on_peak(
    data: &Array2<f32>,
    param_idx: usize,
    bins: usize,
    peak_index: usize,
) -> Option<RangeGate> {
    let peaks = find_peaks(data, param_idx, bins);
    peaks.get(peak_index).map(|peak| peak.gate(param_idx))
}

// ---------------------------------------------------------------------------
// Bisector / BifurGate (Otsu threshold on a bimodal parameter)
// ---------------------------------------------------------------------------

/// The two populations produced by [`bifurcate`].
#[derive(Debug, Clone, PartialEq)]
pub struct Bifurcation {
    /// Threshold in data units.
    pub threshold: f32,
    /// Population below the threshold.
    pub low: RangeGate,
    /// Population at or above the threshold.
    pub high: RangeGate,
    /// Normalised Otsu between-class variance in `[0, 1]`; higher means the
    /// two groups are better separated.
    pub separation: f64,
}

/// Split a 1D parameter into low/high populations at the Otsu threshold.
///
/// This is the 1D "BifurGate"/bisector: it finds the threshold that maximises
/// the between-class variance of the histogram, which is robust for bimodal
/// data. The low gate includes the minimum and excludes the threshold; the
/// high gate includes both the threshold and the maximum, so the two gates
/// partition the data (aside from events outside the finite range).
///
/// Returns `None` when there are no finite values, only one distinct value, or
/// all events fall in a single histogram bin.
pub fn bifurcate(data: &Array2<f32>, param_idx: usize, bins: usize) -> Option<Bifurcation> {
    let values: Vec<f64> = (0..data.nrows())
        .filter_map(|i| data.get((i, param_idx)).copied())
        .filter(|v| v.is_finite())
        .map(|v| v as f64)
        .collect();
    let (counts, min, width) = histogram(&values, bins.max(2))?;
    if counts.len() < 2 {
        return None;
    }
    let total: f64 = counts.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
    if variance <= 0.0 {
        return None;
    }

    let mut best: Option<(usize, f64)> = None;
    let mut sum_low = 0.0;
    let mut weighted_low = 0.0;
    for (t, &count) in counts.iter().enumerate().take(counts.len() - 1) {
        sum_low += count;
        weighted_low += count * (min + (t as f64 + 0.5) * width);
        let sum_high = total - sum_low;
        if sum_low == 0.0 || sum_high == 0.0 {
            continue;
        }
        let mean_low = weighted_low / sum_low;
        let weighted_high: f64 = counts
            .iter()
            .enumerate()
            .skip(t + 1)
            .map(|(i, &c)| c * (min + (i as f64 + 0.5) * width))
            .sum();
        let mean_high = weighted_high / sum_high;
        let between = sum_low * sum_high * (mean_low - mean_high).powi(2);
        if best.is_none_or(|(_, b)| between > b) {
            best = Some((t, between));
        }
    }
    let (split_bin, between) = best?;
    let threshold = (min + (split_bin as f64 + 1.0) * width) as f32;
    let data_min = values.iter().copied().fold(f64::INFINITY, f64::min) as f32;
    let data_max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max) as f32;
    Some(Bifurcation {
        threshold,
        low: RangeGate::new(param_idx, data_min, threshold).with_bounds(true, false),
        high: RangeGate::new(param_idx, threshold, data_max).with_bounds(true, true),
        separation: (between / (total * total * variance)).clamp(0.0, 1.0),
    })
}

// ---------------------------------------------------------------------------
// Cluster gates (deterministic k-means + convex hulls)
// ---------------------------------------------------------------------------

/// One k-means cluster and its enclosing polygon.
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    /// Cluster centre in data coordinates.
    pub center: [f64; 2],
    /// Original event indices belonging to the cluster.
    pub members: Vec<usize>,
    /// Convex hull of the cluster's points (data coordinates).
    pub polygon: Vec<(f32, f32)>,
}

/// Deterministic k-means clustering on two parameters.
///
/// Initial centres are chosen with farthest-point seeding (the first centre is
/// the data mean, each subsequent centre is the point farthest from all
/// existing centres), so no random seed is needed. Lloyd iterations run until
/// assignments stabilise or `iterations` is reached (clamped to at least 1).
///
/// Clusters are sorted by centre x then y, and empty clusters are omitted. The
/// `polygon` field is the convex hull of each cluster's events.
pub fn kmeans_clusters(
    data: &Array2<f32>,
    indices: &[usize],
    x_idx: usize,
    y_idx: usize,
    k: usize,
    iterations: usize,
) -> Vec<Cluster> {
    let points = finite_points(data, indices, x_idx, y_idx);
    if points.is_empty() || k == 0 {
        return Vec::new();
    }
    let k = k.min(points.len());
    let mut centers: Vec<[f64; 2]> = Vec::with_capacity(k);
    let mean = points.iter().fold([0.0f64, 0.0f64], |acc, (_, x, y)| {
        [acc[0] + *x as f64, acc[1] + *y as f64]
    });
    centers.push([mean[0] / points.len() as f64, mean[1] / points.len() as f64]);
    while centers.len() < k {
        let (best_idx, best_dist) = points
            .iter()
            .enumerate()
            .map(|(i, (_, x, y))| {
                let p = [*x as f64, *y as f64];
                let nearest = centers
                    .iter()
                    .map(|c| (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2))
                    .fold(f64::INFINITY, f64::min);
                (i, nearest)
            })
            .fold((0usize, f64::NEG_INFINITY), |best, cur| {
                if cur.1 > best.1 { cur } else { best }
            });
        let (_, x, y) = points[best_idx];
        centers.push([x as f64, y as f64]);
        if best_dist <= 0.0 {
            break;
        }
    }

    let iterations = iterations.max(1);
    let mut assignments: Vec<usize> = vec![usize::MAX; points.len()];
    for _ in 0..iterations {
        let mut changed = false;
        for (i, (_, x, y)) in points.iter().enumerate() {
            let p = [*x as f64, *y as f64];
            let mut best = 0usize;
            let mut best_dist = f64::INFINITY;
            for (c, center) in centers.iter().enumerate() {
                let d = (p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2);
                if d < best_dist {
                    best_dist = d;
                    best = c;
                }
            }
            if assignments[i] != best {
                assignments[i] = best;
                changed = true;
            }
        }
        let mut sums = vec![[0.0f64, 0.0f64]; centers.len()];
        let mut counts = vec![0usize; centers.len()];
        for (i, (_, x, y)) in points.iter().enumerate() {
            let c = assignments[i];
            sums[c][0] += *x as f64;
            sums[c][1] += *y as f64;
            counts[c] += 1;
        }
        for (c, center) in centers.iter_mut().enumerate() {
            if counts[c] > 0 {
                *center = [sums[c][0] / counts[c] as f64, sums[c][1] / counts[c] as f64];
            }
        }
        if !changed {
            break;
        }
    }

    let mut clusters: Vec<Cluster> = (0..centers.len())
        .filter_map(|c| {
            let members: Vec<(usize, f32, f32)> = points
                .iter()
                .enumerate()
                .filter(|(i, _)| assignments[*i] == c)
                .map(|(_, p)| *p)
                .collect();
            if members.is_empty() {
                return None;
            }
            let polygon =
                convex_hull(&members.iter().map(|(_, x, y)| (*x, *y)).collect::<Vec<_>>());
            Some(Cluster {
                center: centers[c],
                members: members.iter().map(|(i, _, _)| *i).collect(),
                polygon,
            })
        })
        .collect();
    clusters.sort_by(|a, b| {
        a.center[0]
            .partial_cmp(&b.center[0])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.center[1]
                    .partial_cmp(&b.center[1])
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    clusters
}

/// Run [`kmeans_clusters`] and turn each cluster hull into a polygon gate.
///
/// Clusters with fewer than three hull vertices are skipped, since a polygon
/// gate needs at least three points.
pub fn cluster_gates(
    data: &Array2<f32>,
    indices: &[usize],
    x_idx: usize,
    y_idx: usize,
    k: usize,
    iterations: usize,
    padding: f32,
) -> Vec<PolygonGate> {
    kmeans_clusters(data, indices, x_idx, y_idx, k, iterations)
        .into_iter()
        .filter(|c| c.polygon.len() >= 3)
        .map(|c| {
            let (cx, cy) = c
                .polygon
                .iter()
                .fold((0.0f64, 0.0f64), |(sx, sy), &(x, y)| {
                    (sx + x as f64, sy + y as f64)
                });
            let cx = (cx / c.polygon.len() as f64) as f32;
            let cy = (cy / c.polygon.len() as f64) as f32;
            let scale = 1.0 + padding.max(0.0);
            PolygonGate::new(
                x_idx,
                y_idx,
                c.polygon
                    .iter()
                    .map(|&(x, y)| (cx + (x - cx) * scale, cy + (y - cy) * scale))
                    .collect(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gating::Gate;
    use ndarray::array;

    fn bimodal_1d() -> Array2<f32> {
        // Three low events around 2, three high events around 8.
        array![[1.0], [2.0], [3.0], [7.0], [8.0], [9.0]]
    }

    fn two_blobs() -> Array2<f32> {
        array![
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [10.0, 10.0],
            [11.0, 10.0],
            [10.0, 11.0],
        ]
    }

    #[test]
    fn convex_hull_ignores_interior_and_collinear_points() {
        let points = [
            (0.0f32, 0.0f32),
            (2.0, 0.0),
            (2.0, 2.0),
            (0.0, 2.0),
            (1.0, 1.0), // interior
            (1.0, 0.0), // collinear on the bottom edge
        ];
        let hull = convex_hull(&points);
        assert_eq!(hull.len(), 4);
        assert!(hull.contains(&(0.0, 0.0)));
        assert!(hull.contains(&(2.0, 0.0)));
        assert!(hull.contains(&(2.0, 2.0)));
        assert!(hull.contains(&(0.0, 2.0)));
        assert!(!hull.contains(&(1.0, 1.0)));
    }

    #[test]
    fn polygon_autogate_encloses_population() {
        let data = two_blobs();
        let gate = polygon_autogate(&data, &[0, 1, 2], 0, 1, 0.1).unwrap();
        assert!(gate.vertices.len() >= 3);
        assert_eq!(gate.filter(&data), vec![0, 1, 2]);
        // Padding expands beyond the original extents.
        let min_x = gate
            .vertices
            .iter()
            .map(|v| v.0)
            .fold(f32::INFINITY, f32::min);
        assert!(min_x < 0.0);
        // Too few points cannot make a polygon.
        assert!(polygon_autogate(&data, &[0, 1], 0, 1, 0.0).is_none());
        assert!(polygon_autogate(&data, &[], 0, 1, 0.0).is_none());
    }

    #[test]
    fn find_peaks_locates_both_modes() {
        let data = bimodal_1d();
        let peaks = find_peaks(&data, 0, 16);
        assert!(peaks.len() >= 2, "peaks: {peaks:?}");
        assert!((peaks[0].center - 2.0).abs() <= 1.0, "peaks: {peaks:?}");
        assert!((peaks[1].center - 8.0).abs() <= 1.0, "peaks: {peaks:?}");
        // Each peak's range contains its events.
        let low = peaks[0].gate(0);
        assert_eq!(low.filter(&data), vec![0, 1, 2]);
        let high = peaks[1].gate(0);
        assert_eq!(high.filter(&data), vec![3, 4, 5]);

        let by_gate = gate_on_peak(&data, 0, 16, 1).unwrap();
        assert_eq!(by_gate.filter(&data), vec![3, 4, 5]);
        assert!(gate_on_peak(&data, 0, 16, 9).is_none());

        // A constant parameter yields a single degenerate peak.
        let constant = array![[5.0f32], [5.0], [5.0]];
        let peaks = find_peaks(&constant, 0, 8);
        assert_eq!(peaks.len(), 1);
        assert_eq!(peaks[0].start, 5.0);
        assert_eq!(peaks[0].end, 5.0);
    }

    #[test]
    fn bifurcate_splits_bimodal_parameter() {
        let data = bimodal_1d();
        let split = bifurcate(&data, 0, 16).unwrap();
        assert!(split.threshold > 3.0 && split.threshold < 7.0);
        assert!(split.separation > 0.8);
        assert_eq!(split.low.filter(&data), vec![0, 1, 2]);
        assert_eq!(split.high.filter(&data), vec![3, 4, 5]);

        // Uniform data has no meaningful split.
        let flat = array![[1.0f32], [1.0], [1.0], [1.0]];
        assert!(bifurcate(&flat, 0, 8).is_none());
    }

    #[test]
    fn kmeans_finds_two_blobs_and_builds_gates() {
        let data = two_blobs();
        let clusters = kmeans_clusters(&data, &[0, 1, 2, 3, 4, 5], 0, 1, 2, 50);
        assert_eq!(clusters.len(), 2);
        assert!((clusters[0].center[0] - 1.0 / 3.0).abs() < 1e-6);
        assert!((clusters[0].center[1] - 1.0 / 3.0).abs() < 1e-6);
        assert!((clusters[1].center[0] - 31.0 / 3.0).abs() < 1e-6);
        assert_eq!(clusters[0].members, vec![0, 1, 2]);
        assert_eq!(clusters[1].members, vec![3, 4, 5]);
        assert_eq!(clusters[0].polygon.len(), 3);

        let gates = cluster_gates(&data, &[0, 1, 2, 3, 4, 5], 0, 1, 2, 50, 0.1);
        assert_eq!(gates.len(), 2);
        assert_eq!(gates[0].filter(&data), vec![0, 1, 2]);
        assert_eq!(gates[1].filter(&data), vec![3, 4, 5]);

        // Degenerate inputs.
        assert!(kmeans_clusters(&data, &[], 0, 1, 2, 10).is_empty());
        assert!(kmeans_clusters(&data, &[0, 1, 2], 0, 1, 0, 10).is_empty());
    }
}
