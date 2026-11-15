use ndarray::Array2;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// How a compensation matrix was produced (V5 / C2).
///
/// Mirrors the matrix provenance FlowJo tracks: an acquisition-time spillover
/// matrix, a hand-edited or calculated matrix, or an imported matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum MatrixType {
    /// Recorded from the acquisition (`$SPILLOVER`) matrix.
    #[default]
    Acquisition,
    /// Edited by the user in the compensation editor.
    Edited,
    /// Computed from single-stain controls.
    Calculated,
    /// Imported from an external source.
    Imported,
}

fn default_matrix_type() -> MatrixType {
    MatrixType::Acquisition
}

#[derive(Debug, Error)]
pub enum CompensationError {
    #[error("Matrix dimensions mismatch: expected {expected}, found {found}")]
    DimensionMismatch { expected: String, found: String },
    #[error("Invalid compensation matrix: {0}")]
    InvalidMatrix(String),
    #[error("No matrix channel matches this sample's parameters")]
    NoMatchingChannels,
    #[error("Matrix is singular and cannot be inverted")]
    SingularMatrix,
}

pub struct CompensationEngine {
    /// The compensation matrix (spillover matrix)
    /// Rows = Original channels, Cols = Corrected channels
    pub matrix: Array2<f32>,
}

impl CompensationEngine {
    pub fn new(matrix: Array2<f32>) -> Self {
        Self { matrix }
    }

    /// Applies compensation to a dataset.
    /// data: (events x channels)
    /// result: (events x channels)
    pub fn compensate(&self, data: &Array2<f32>) -> Result<Array2<f32>, CompensationError> {
        let (_events, channels) = data.dim();
        let (mat_rows, _mat_cols) = self.matrix.dim();

        if channels != mat_rows {
            return Err(CompensationError::DimensionMismatch {
                expected: format!("{} channels", mat_rows),
                found: format!("{} channels", channels),
            });
        }

        // Perform matrix multiplication: [Events x Channels] * [Channels x Channels]
        // ndarray's dot product is efficient
        Ok(data.dot(&self.matrix))
    }
}

/// A named compensation / spillover matrix.
///
/// `channels[i]` labels both row `i` and column `i`; `values[row][col]` is the
/// coefficient that maps the row channel into the column channel. Matrices are
/// applied as `data × matrix`, so a matrix loaded from a CSV is used exactly as
/// written. [`CompensationMatrix::inverted`] converts a raw spillover matrix
/// (as stored in an FCS `$SPILLOVER` keyword) into an applied compensation
/// matrix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompensationMatrix {
    pub channels: Vec<String>,
    pub values: Vec<Vec<f32>>,
    /// How this matrix was produced (V5 / C2).
    #[serde(default = "default_matrix_type")]
    pub matrix_type: MatrixType,
}

/// Result of validating a compensation matrix (V5 / C2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompensationValidation {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl CompensationMatrix {
    /// Validates matrix contents: finite values, invertibility (conditioning).
    ///
    /// Shape validity (square, non-empty) is enforced by [`CompensationMatrix::new`]
    /// and [`CompensationMatrix::from_csv`]; this pass covers value-level checks.
    pub fn validate(&self) -> CompensationValidation {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let n = self.channels.len();
        if n == 0 {
            return CompensationValidation {
                ok: false,
                errors: vec!["the matrix has no channels".to_string()],
                warnings,
            };
        }
        for (i, row) in self.values.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                if !v.is_finite() {
                    errors.push(format!("value at row {i}, column {j} is not finite"));
                }
            }
        }
        if self.inverted().is_err() {
            errors.push("the matrix is singular and cannot be inverted".to_string());
        }
        for i in 0..n {
            let d = self.values[i][i];
            if (d - 1.0).abs() > 1e-4 {
                warnings.push(format!("diagonal entry {i} is {d}, expected 1"));
            }
        }
        CompensationValidation {
            ok: errors.is_empty(),
            errors,
            warnings,
        }
    }
}

/// Cell-level comparison between two compensation matrices (V5 / C2).
#[derive(Debug, Clone, PartialEq)]
pub struct CompensationComparison {
    pub same_channels: bool,
    pub max_abs_difference: f32,
    pub identical: bool,
}

impl CompensationMatrix {
    /// Compares two matrices: channel sets must agree and every coefficient
    /// must match within `1e-6` to be considered identical.
    pub fn compare(&self, other: &Self) -> CompensationComparison {
        let same_channels = self.channels == other.channels;
        let mut identical = true;
        let mut max_abs_difference = 0.0_f32;
        if self.values.len() == other.values.len()
            && self
                .values
                .iter()
                .zip(&other.values)
                .all(|(ra, rb)| ra.len() == rb.len())
        {
            for (ra, rb) in self.values.iter().zip(&other.values) {
                for (a, b) in ra.iter().zip(rb.iter()) {
                    let d = (*a - *b).abs();
                    if d > max_abs_difference {
                        max_abs_difference = d;
                    }
                    if d > 1e-6 {
                        identical = false;
                    }
                }
            }
        } else {
            identical = false;
        }
        CompensationComparison {
            same_channels,
            max_abs_difference,
            identical,
        }
    }
}

impl CompensationMatrix {
    /// Build a matrix from channel names and rows, validating that it is square.
    pub fn new(channels: Vec<String>, values: Vec<Vec<f32>>) -> Result<Self, CompensationError> {
        let n = channels.len();
        if n == 0 {
            return Err(CompensationError::InvalidMatrix(
                "the matrix has no channels".to_string(),
            ));
        }
        if values.len() != n {
            return Err(CompensationError::InvalidMatrix(format!(
                "expected {n} rows, found {}",
                values.len()
            )));
        }
        for (i, row) in values.iter().enumerate() {
            if row.len() != n {
                return Err(CompensationError::InvalidMatrix(format!(
                    "row {} has {} columns, expected {n}",
                    i + 1,
                    row.len()
                )));
            }
        }
        Ok(Self {
            channels,
            values,
            matrix_type: MatrixType::Acquisition,
        })
    }

    /// An identity matrix for the given channels.
    pub fn identity(channels: Vec<String>) -> Result<Self, CompensationError> {
        let n = channels.len();
        let values = (0..n)
            .map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect())
            .collect();
        Self::new(channels, values)
    }

    /// Parses a CSV/TSV/semicolon-separated matrix.
    ///
    /// Both header layouts commonly exported by flow-cytometry software are
    /// accepted:
    ///
    /// ```text
    /// ,FITC-A,PE-A        FITC-A,1,0.02
    /// FITC-A,1,0.02       PE-A,0.03,1
    /// PE-A,0.03,1
    /// ```
    ///
    /// A headerless square matrix is also accepted; its channels are named
    /// `P1`, `P2`, … and can be remapped in the GUI.
    pub fn from_csv(text: &str) -> Result<Self, CompensationError> {
        let text = text.trim_start_matches('\u{feff}');
        let mut lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        if lines.is_empty() {
            return Err(CompensationError::InvalidMatrix(
                "the file is empty".to_string(),
            ));
        }

        // Pick the delimiter from the header / first data row.
        let delimiter = [',', '\t', ';']
            .into_iter()
            .max_by_key(|d| lines[0].matches(*d).count())
            .unwrap_or(',');

        let cells: Vec<Vec<String>> = lines
            .drain(..)
            .map(|line| {
                line.split(delimiter)
                    .map(|s| s.trim().trim_matches('"').trim().to_string())
                    .collect::<Vec<_>>()
            })
            .collect();

        let is_number = |s: &str| s.parse::<f32>().is_ok();
        let first_row_numeric = cells[0].iter().filter(|s| is_number(s)).count();

        let (channels, data_rows): (Vec<String>, Vec<Vec<String>>) = if first_row_numeric == 0 {
            // First row contains channel names.
            let mut header = cells[0].clone();
            let mut rows = cells[1..].to_vec();

            // Drop a corner label / row-label column when present.
            let has_corner = header.first().is_some_and(|s| !is_number(s));
            let data_width_without_label = rows
                .first()
                .map(|r| {
                    if r.first().is_some_and(|s| !is_number(s)) {
                        r.len().saturating_sub(1)
                    } else {
                        r.len()
                    }
                })
                .unwrap_or(0);
            if has_corner && header.len().saturating_sub(1) == data_width_without_label {
                header.remove(0);
            }

            let row_labels = rows
                .first()
                .is_some_and(|r| r.first().is_some_and(|s| !is_number(s)));
            if row_labels {
                for row in &mut rows {
                    if row.first().is_some_and(|s| !is_number(s)) {
                        row.remove(0);
                    }
                }
            }
            (header, rows)
        } else {
            // No header row: the first row is data. If its first cell is a name,
            // it is a row label and the channels come from the first column.
            let row_labels = cells[0]
                .first()
                .is_some_and(|s| !s.is_empty() && !is_number(s));
            let mut rows = cells;
            let mut channels = Vec::new();
            if row_labels {
                for row in &mut rows {
                    if let Some(label) = row.first().cloned() {
                        channels.push(label);
                        row.remove(0);
                    }
                }
            }
            if channels.len() != rows.len() {
                channels = (1..=rows.first().map(Vec::len).unwrap_or(0))
                    .map(|i| format!("P{i}"))
                    .collect();
            }
            (channels, rows)
        };

        let width = data_rows.first().map(Vec::len).unwrap_or(0);
        if width == 0 {
            return Err(CompensationError::InvalidMatrix(
                "the matrix has no numeric values".to_string(),
            ));
        }
        if data_rows.len() != width {
            return Err(CompensationError::InvalidMatrix(format!(
                "the matrix must be square: {} rows x {width} columns",
                data_rows.len()
            )));
        }
        let mut channels = channels;
        if channels.len() != width {
            channels = (1..=width).map(|i| format!("P{i}")).collect();
        }

        let mut values = Vec::with_capacity(data_rows.len());
        for (r, row) in data_rows.iter().enumerate() {
            let mut out = Vec::with_capacity(width);
            for (c, cell) in row.iter().enumerate() {
                let v = cell.parse::<f32>().map_err(|_| {
                    CompensationError::InvalidMatrix(format!(
                        "row {} column {} is not a number: '{cell}'",
                        r + 1,
                        c + 1
                    ))
                })?;
                out.push(v);
            }
            values.push(out);
        }
        Self::new(channels, values)
    }

    /// Parses an FCS `$SPILLOVER` keyword (`n,ch1,…,chn,v11,v12,…`).
    pub fn from_spillover_keyword(text: &str) -> Result<Self, CompensationError> {
        let parts: Vec<&str> = text
            .split([',', ';'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let n = parts
            .first()
            .and_then(|s| s.parse::<usize>().ok())
            .ok_or_else(|| {
                CompensationError::InvalidMatrix(
                    "$SPILLOVER is missing its channel count".to_string(),
                )
            })?;
        if n == 0 || parts.len() < 1 + n + n * n {
            return Err(CompensationError::InvalidMatrix(format!(
                "$SPILLOVER should contain {n} channels and {} values",
                n * n
            )));
        }
        let channels: Vec<String> = parts[1..=n].iter().map(|s| s.to_string()).collect();
        let mut values = Vec::with_capacity(n);
        let mut pos = 1 + n;
        for _ in 0..n {
            let mut row = Vec::with_capacity(n);
            for _ in 0..n {
                row.push(parts[pos].parse::<f32>().map_err(|_| {
                    CompensationError::InvalidMatrix(format!(
                        "bad $SPILLOVER value '{}'",
                        parts[pos]
                    ))
                })?);
                pos += 1;
            }
            values.push(row);
        }
        Self::new(channels, values)
    }

    /// Inverts the matrix with Gauss-Jordan elimination.
    ///
    /// FCS `$SPILLOVER` stores the raw spillover matrix; applying compensation
    /// requires its inverse.
    pub fn inverted(&self) -> Result<Self, CompensationError> {
        let n = self.channels.len();
        let mut a: Vec<Vec<f64>> = self
            .values
            .iter()
            .map(|r| r.iter().map(|v| *v as f64).collect())
            .collect();
        let mut inv: Vec<Vec<f64>> = (0..n)
            .map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect())
            .collect();

        for col in 0..n {
            let pivot = (col..n)
                .max_by(|&r1, &r2| {
                    a[r1][col]
                        .abs()
                        .partial_cmp(&a[r2][col].abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(col);
            if a[pivot][col].abs() < 1e-12 {
                return Err(CompensationError::SingularMatrix);
            }
            if pivot != col {
                a.swap(pivot, col);
                inv.swap(pivot, col);
            }
            let d = a[col][col];
            for j in 0..n {
                a[col][j] /= d;
                inv[col][j] /= d;
            }
            for r in 0..n {
                if r == col {
                    continue;
                }
                let f = a[r][col];
                if f.abs() < 1e-18 {
                    continue;
                }
                for j in 0..n {
                    a[r][j] -= f * a[col][j];
                    inv[r][j] -= f * inv[col][j];
                }
            }
        }

        Self::new(
            self.channels.clone(),
            inv.into_iter()
                .map(|r| r.into_iter().map(|v| v as f32).collect())
                .collect(),
        )
    }

    /// Computes a compensation matrix from single-stain control median
    /// profiles and marks it `Calculated` (V5 / C1).
    ///
    /// `profiles[j][i]` is the median fluorescence of channel `i` in single-stain
    /// control `j`; [`spreading_matrix`] turns those into the raw spillover
    /// `S`, and the applied compensation matrix is `S⁻¹`.
    pub fn from_control_profiles(profiles: &[Vec<f32>]) -> Result<Self, CompensationError> {
        let spillover = spreading_matrix(profiles)?;
        let n = spillover.nrows();
        let channels = (1..=n).map(|i| format!("P{i}")).collect();
        let values: Vec<Vec<f32>> = (0..n)
            .map(|i| (0..n).map(|j| spillover[[i, j]]).collect())
            .collect();
        let matrix = Self::new(channels, values)?;
        let applied = matrix.inverted()?;
        let mut out = applied;
        out.matrix_type = MatrixType::Calculated;
        Ok(out)
    }

    /// Index of the matrix channel in the sample, matching `$PnN` names first
    /// and `$PnS` descriptions second (case-insensitive).
    fn channel_index(channel: &str, names: &[String], descriptions: &[String]) -> Option<usize> {
        let wanted = channel.trim();
        names
            .iter()
            .position(|n| n.trim().eq_ignore_ascii_case(wanted))
            .or_else(|| {
                descriptions
                    .iter()
                    .position(|d| d.trim().eq_ignore_ascii_case(wanted))
            })
    }

    /// Number of matrix channels that can be mapped onto this sample.
    pub fn matched_count(&self, names: &[String], descriptions: &[String]) -> usize {
        self.channels
            .iter()
            .filter(|c| Self::channel_index(c, names, descriptions).is_some())
            .count()
    }

    /// Applies the matrix to a full event × parameter data matrix.
    ///
    /// Channels that are absent from the matrix keep an identity row/column, so
    /// a fluorescence-only matrix can be applied to a file that also contains
    /// scatter and time channels.
    pub fn apply_to(
        &self,
        data: &Array2<f32>,
        names: &[String],
        descriptions: &[String],
    ) -> Result<Array2<f32>, CompensationError> {
        let channels = data.ncols();
        if names.len() != channels {
            return Err(CompensationError::DimensionMismatch {
                expected: format!("{channels} parameter names"),
                found: format!("{} parameter names", names.len()),
            });
        }

        let n = self.channels.len();
        let mut full = Array2::<f32>::eye(channels);
        let mut matched = 0usize;
        for (i, channel) in self.channels.iter().enumerate() {
            let Some(ci) = Self::channel_index(channel, names, descriptions) else {
                continue;
            };
            matched += 1;
            for (j, target) in self.channels.iter().enumerate() {
                if let Some(cj) = Self::channel_index(target, names, descriptions) {
                    full[[ci, cj]] = self.values[i][j];
                }
            }
        }

        if matched == 0 {
            // Positional fallback: a headerless matrix of exactly the right size.
            if n == channels {
                for i in 0..n {
                    for j in 0..n {
                        full[[i, j]] = self.values[i][j];
                    }
                }
            } else {
                return Err(CompensationError::NoMatchingChannels);
            }
        }

        CompensationEngine::new(full).compensate(data)
    }
}

/// Cosine similarity between two channel-intensity signatures.
///
/// Returns `1.0` for identical directions, `0.0` for orthogonal vectors and
/// `0.0` when either vector is (near) zero.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let na = a.iter().map(|v| v * v).sum::<f32>().sqrt();
    let nb = b.iter().map(|v| v * v).sum::<f32>().sqrt();
    if na <= f32::EPSILON || nb <= f32::EPSILON {
        return 0.0;
    }
    let dot = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>();
    dot / (na * nb)
}

/// Spreading (spillover-spread) matrix derived from single-stain control
/// profiles: `profiles[j][i]` is the median fluorescence of channel `i` in
/// single-stain control `j`. Entry `[i][j]` is the spillover of channel `i`
/// into channel `j`, normalized so the primary channel of control `j` is `1.0`.
pub fn spreading_matrix(profiles: &[Vec<f32>]) -> Result<Array2<f32>, CompensationError> {
    let n = profiles.len();
    if n == 0 || profiles.iter().any(|p| p.len() != n) {
        return Err(CompensationError::InvalidMatrix(
            "spreading matrices need square single-stain profiles".to_string(),
        ));
    }
    let medians: Vec<Vec<f32>> = profiles
        .iter()
        .map(|p| p.iter().map(|v| v.max(0.0)).collect::<Vec<f32>>())
        .collect();
    let mut out = Array2::<f32>::zeros((n, n));
    for j in 0..n {
        let primary = medians[j][j];
        if primary <= 0.0 {
            return Err(CompensationError::InvalidMatrix(
                "single-stain profile has a zero primary channel".to_string(),
            ));
        }
        for i in 0..n {
            out[[i, j]] = medians[i][j] / primary;
        }
    }
    Ok(out)
}

/// Total spillover spread for each control: the sum of the normalized
/// spreading coefficients `S[i][j]` over all detector channels `i`.
pub fn total_spread(spread: &Array2<f32>) -> f32 {
    let mut total = 0.0_f32;
    for col in 0..spread.ncols() {
        let mut row_sum = 0.0_f32;
        for row in 0..spread.nrows() {
            row_sum += spread[[row, col]];
        }
        total += row_sum;
    }
    total
}

/// Solves the linear model `y = Σ_k w_k · source_k` for every measured
/// spectrum via ordinary least squares (C5): builds the design matrix
/// `M[j][k] = source_k[j]` and solves the normal equations `MᵀM·w = Mᵀy` for each
/// mixture, returning one fitted weight vector per measured spectrum.
pub fn solve_ols(
    sources: &[Vec<f32>],
    mixtures: &[Vec<f32>],
) -> Result<Vec<Vec<f32>>, CompensationError> {
    let k = sources.len();
    let n = sources.first().map(|s| s.len()).unwrap_or(0);
    if k == 0 || n == 0 {
        return Err(CompensationError::InvalidMatrix(
            "spectral unmixing needs at least one source and one detector value".to_string(),
        ));
    }
    for source in sources {
        if source.len() != n {
            return Err(CompensationError::InvalidMatrix(
                "spectral sources must share the detector count".to_string(),
            ));
        }
    }
    for y in mixtures {
        if y.len() != n {
            return Err(CompensationError::InvalidMatrix(
                "spectral mixtures must share the detector count".to_string(),
            ));
        }
    }
    let mut results = Vec::with_capacity(mixtures.len());
    for y in mixtures {
        // Normal equations: (MᵀM)·w = Mᵀ·y with M[j][k] = sources[k][j].
        let mut ata = vec![vec![0.0_f64; k]; k];
        let mut aty = vec![0.0_f64; k];
        for j in 0..n {
            let col: Vec<f64> = (0..k).map(|kk| sources[kk][j] as f64).collect();
            for a in 0..k {
                for b in 0..k {
                    ata[a][b] += col[a] * col[b];
                }
                aty[a] += col[a] * y[j] as f64;
            }
        }
        // Gaussian elimination with partial pivoting on the normal equations.
        let mut m = ata;
        let mut rhs = aty;
        for col in 0..k {
            let pivot = (col..k)
                .max_by(|&r1, &r2| {
                    m[r1][col]
                        .abs()
                        .partial_cmp(&m[r2][col].abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(col);
            if m[pivot][col].abs() < 1e-12 {
                return Err(CompensationError::SingularMatrix);
            }
            if pivot != col {
                m.swap(col, pivot);
                rhs.swap(col, pivot);
            }
            let d = m[col][col];
            for value in &mut m[col][col..k] {
                *value /= d;
            }
            rhs[col] /= d;
            for r in 0..k {
                if r == col {
                    continue;
                }
                let f = m[r][col];
                if f.abs() < 1e-18 {
                    continue;
                }
                let source = m[col][col..k].to_vec();
                for (b, value) in m[r][col..k].iter_mut().enumerate() {
                    *value -= f * source[b];
                }
                rhs[r] -= f * rhs[col];
            }
        }
        results.push(rhs.into_iter().map(|v| v as f32).collect());
    }
    Ok(results)
}

/// Result of unmixing one measured spectrum against known sources (C5).
#[derive(Debug, Clone, PartialEq)]
pub struct SpectralResult {
    /// Index of the mixture (source combination) that was unmixable.
    pub mixture: usize,
    /// Fitted source weights, one per source.
    pub weights: Vec<f32>,
    /// Residual: norm of the part of the mixture not explained by the sources.
    pub residual: f32,
}

/// Unmix measured spectra against known sources via ordinary least squares
/// (BD SpectralFX-style), returning one [`SpectralResult`] per mixture (C5).
pub fn unmix(
    sources: &[Vec<f32>],
    mixtures: &[Vec<f32>],
) -> Result<Vec<SpectralResult>, CompensationError> {
    let k = sources.len();
    let n = sources.first().map(|s| s.len()).unwrap_or(0);
    if k == 0 || n == 0 {
        return Err(CompensationError::InvalidMatrix(
            "spectral unmixing needs at least one source and one detector value".to_string(),
        ));
    }
    let mut results = Vec::with_capacity(mixtures.len());
    for (m, y) in mixtures.iter().enumerate() {
        let weight_vecs = solve_ols(sources, std::slice::from_ref(y))?;
        let weight_vec = weight_vecs
            .into_iter()
            .next()
            .ok_or(CompensationError::InvalidMatrix(
                "empty unmixing result".to_string(),
            ))?;
        let mut pred = vec![0.0_f64; n];
        for (kk, src) in sources.iter().enumerate() {
            for j in 0..n {
                pred[j] += weight_vec[kk] as f64 * src[j] as f64;
            }
        }
        let residual: f64 = (0..n).map(|j| (y[j] as f64 - pred[j]).powi(2).sqrt()).sum();
        results.push(SpectralResult {
            mixture: m,
            weights: weight_vec.clone(),
            residual: residual as f32,
        });
    }
    Ok(results)
}

/// Applies a compensation engine to every sample data matrix in a group of
/// samples (C6 group-wide apply): returns one compensated matrix per input,
/// preserving order. `remove` is expressed by simply not calling this on the
/// members that should stay uncompensated. (Group-wide apply/remove.)
pub fn apply_group(
    engine: &CompensationEngine,
    data: &[Array2<f32>],
) -> Result<Vec<Array2<f32>>, CompensationError> {
    data.iter().map(|d| engine.compensate(d)).collect()
}

/// Imports spectral mixture weights from a CSV text: one row per mixture,
/// comma-separated source weights (C5 spectral-weight import/export).
pub fn import_weights_csv(text: &str) -> Result<Vec<Vec<f32>>, CompensationError> {
    let mut out = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let row: Vec<f32> = line
            .split(',')
            .map(|t| t.trim().parse::<f32>())
            .collect::<Result<Vec<f32>, _>>()
            .map_err(|_| CompensationError::InvalidMatrix(format!("bad weight row {line_no}")))?;
        if row.is_empty() {
            return Err(CompensationError::InvalidMatrix(format!(
                "empty weight row {line_no}"
            )));
        }
        out.push(row);
    }
    Ok(out)
}

/// Exports spectral mixture weights to CSV text: one row per mixture, the
/// comma-separated source weights (C5 spectral-weight export).
pub fn export_weights_csv(results: &[SpectralResult]) -> String {
    results
        .iter()
        .map(|r| {
            r.weights
                .iter()
                .map(|w| format!("{w}"))
                .collect::<Vec<String>>()
                .join(",")
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Optimize source weights for a set of measured mixtures by deterministic
/// non-negative gradient refinement of the residual sum of squares
/// (C4 weight optimization).
///
/// Starts from the ordinary-least-squares solution and refines the weights
/// with a fixed-step gradient update clamped to the non-negative orthant, so
/// the result is fully deterministic for a given input.
pub fn optimize_weights(
    sources: &[Vec<f32>],
    mixtures: &[Vec<f32>],
    iterations: usize,
) -> Result<Vec<Vec<f32>>, CompensationError> {
    let k = sources.len();
    let n = sources.first().map(|s| s.len()).unwrap_or(0);
    if k == 0 || n == 0 {
        return Err(CompensationError::InvalidMatrix(
            "spectral weight optimization needs at least one source and one detector value"
                .to_string(),
        ));
    }
    for source in sources {
        if source.len() != n {
            return Err(CompensationError::InvalidMatrix(
                "spectral sources must share the detector count".to_string(),
            ));
        }
    }
    let lr = 0.5_f32;
    let ridge = 1e-6_f32;
    let mut out = Vec::with_capacity(mixtures.len());
    for y in mixtures {
        if y.len() != n {
            return Err(CompensationError::InvalidMatrix(
                "spectral mixtures must share the detector count".to_string(),
            ));
        }
        let mut w = solve_ols(sources, std::slice::from_ref(y))?
            .into_iter()
            .next()
            .ok_or(CompensationError::InvalidMatrix(
                "empty unmixing result".to_string(),
            ))?;
        for _ in 0..iterations.max(1) {
            // residual = y - W * S
            let mut pred = vec![0.0_f32; n];
            for (j, source) in sources.iter().enumerate() {
                for t in 0..n {
                    pred[t] += w[j] * source[t];
                }
            }
            let mut grad = vec![0.0_f32; k];
            for j in 0..k {
                let mut g = 0.0_f32;
                for t in 0..n {
                    g += sources[j][t] * (y[t] - pred[t]);
                }
                grad[j] = -2.0 * g;
            }
            for j in 0..k {
                w[j] = ((w[j] - lr * grad[j]) / (1.0 + lr * ridge)).max(0.0);
            }
        }
        out.push(w);
    }
    Ok(out)
}

/// Raw-versus-compensated overlay data (C4 compensation animation/overlays):
/// the original raw values plus the compensated result, both kept for plotting
/// overlays that compare raw and compensated populations.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayData {
    pub raw: Array2<f32>,
    pub compensated: Array2<f32>,
}

/// Compute the raw-versus-compensated overlay for one data matrix by applying
/// the engine (C4 raw-versus-compensated overlays).
pub fn raw_overlay(
    engine: &CompensationEngine,
    data: &Array2<f32>,
) -> Result<OverlayData, CompensationError> {
    let compensated = engine.compensate(data)?;
    Ok(OverlayData {
        raw: data.clone(),
        compensated,
    })
}

/// Animation frames for a compensation matrix: `frames` evenly spaced
/// interpolated copies between `from` and `to` (C4 compensation animation).
///
/// Frame 0 is `from`, the last frame is `to`, and intermediate frames are the
/// linear interpolation of the matrix entries.
pub fn animate_matrix(from: &Array2<f32>, to: &Array2<f32>, frames: usize) -> Vec<Array2<f32>> {
    let (r0, c0) = from.dim();
    let (r1, c1) = to.dim();
    if frames == 0 || r0 != r1 || c0 != c1 {
        return vec![from.clone()];
    }
    if frames == 1 {
        return vec![from.clone()];
    }
    let mut out = Vec::with_capacity(frames);
    for i in 0..frames {
        let t = i as f32 / (frames - 1) as f32;
        let mut m = ndarray::Array2::<f32>::zeros((r0, c0));
        for r in 0..r0 {
            for c in 0..c0 {
                m[[r, c]] = from[[r, c]] + (to[[r, c]] - from[[r, c]]) * t;
            }
        }
        out.push(m);
    }
    out
}

/// Result of the compensation wizard (C7): the spillover matrix calculated
/// from the assigned single-stain control median profiles, the derived
/// compensation matrix (`S⁻¹`) and the total spillover spread.
#[derive(Debug, Clone, PartialEq)]
pub struct CompensationWizardResult {
    /// The compensation matrix (`S⁻¹`) the wizard derives and previews.
    pub matrix: CompensationMatrix,
    /// Raw spreading (spillover) matrix `S` from the single-stain profiles.
    pub spillover: Array2<f32>,
    /// Total spillover spread across the single-stain controls.
    pub total_spread: f32,
}

/// Run the compensation wizard over the assigned single-stain control median
/// profiles (C7): calculate the spillover matrix, derive the compensation
/// matrix and report the total spread for preview.
pub fn compensation_wizard(
    profiles: &[Vec<f32>],
) -> Result<CompensationWizardResult, CompensationError> {
    let spillover = spreading_matrix(profiles)?;
    let total = total_spread(&spillover);
    let matrix = CompensationMatrix::from_control_profiles(profiles)?;
    Ok(CompensationWizardResult {
        matrix,
        spillover,
        total_spread: total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn compensation_wizard_derives_matrix_and_spread() {
        let profiles = vec![vec![10.0, 2.0], vec![5.0, 40.0]];
        let w = compensation_wizard(&profiles).unwrap();
        assert_eq!(w.matrix.matrix_type, MatrixType::Calculated);
        assert!((w.spillover[[0, 0]] - 1.0).abs() < 1e-6);
        assert!((w.total_spread - 2.55).abs() < 1e-6);
    }

    #[test]
    fn compensation_wizard_rejects_empty_control_profiles() {
        assert!(compensation_wizard(&[]).is_err());
    }

    #[test]
    fn csv_with_header_and_row_labels_parses() {
        let m = CompensationMatrix::from_csv(",FITC-A,PE-A\nFITC-A,1,0.02\nPE-A,0.03,1\n").unwrap();
        assert_eq!(m.channels, vec!["FITC-A", "PE-A"]);
        assert_eq!(m.values, vec![vec![1.0, 0.02], vec![0.03, 1.0]]);
    }

    #[test]
    fn csv_without_corner_label_parses() {
        let m = CompensationMatrix::from_csv("FITC-A,PE-A\n1,0.02\n0.03,1").unwrap();
        assert_eq!(m.channels, vec!["FITC-A", "PE-A"]);
        assert_eq!(m.values[1][0], 0.03);
    }

    #[test]
    fn headerless_csv_gets_positional_names() {
        let m = CompensationMatrix::from_csv("1,0\n0,1").unwrap();
        assert_eq!(m.channels, vec!["P1", "P2"]);
    }

    #[test]
    fn apply_maps_named_channels_onto_all_parameters() {
        let m = CompensationMatrix::from_csv(",FITC-A,PE-A\nFITC-A,1,-0.1\nPE-A,-0.2,1").unwrap();
        let data = array![[10.0_f32, 100.0, 5.0], [20.0, 200.0, 6.0]];
        let names = vec!["FSC-A".into(), "FITC-A".into(), "PE-A".into()];
        let desc = names.clone();
        let out = m.apply_to(&data, &names, &desc).unwrap();
        // FSC is untouched by the identity row/column.
        assert!((out[[0, 0]] - 10.0).abs() < 1e-5);
        // FITC = 100*1 + 5*(-0.2) = 99.
        assert!((out[[0, 1]] - 99.0).abs() < 1e-5);
        // PE = 100*(-0.1) + 5*1 = -5.
        assert!((out[[0, 2]] - (-5.0)).abs() < 1e-5);
    }

    #[test]
    fn spillover_keyword_parses_and_inverts() {
        let raw = CompensationMatrix::from_spillover_keyword("2,FITC-A,PE-A,1,0.1,0.2,1").unwrap();
        assert_eq!(raw.channels, vec!["FITC-A", "PE-A"]);
        let inv = raw.inverted().unwrap();
        // raw × inverse = identity.
        for i in 0..2 {
            for j in 0..2 {
                let v: f32 = (0..2).map(|k| raw.values[i][k] * inv.values[k][j]).sum();
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((v - expected).abs() < 1e-4, "{i},{j}: {v}");
            }
        }
    }

    #[test]
    fn from_control_profiles_builds_calculated_inverse() {
        // Control A: primary 10, contaminant 2. Control B: primary 40, contaminant 5.
        let profiles = vec![vec![10.0, 2.0], vec![5.0, 40.0]];
        let m = CompensationMatrix::from_control_profiles(&profiles).unwrap();
        assert_eq!(m.matrix_type, MatrixType::Calculated);

        // Raw = true * S, compensated = raw * S^-1 == true.
        let s = spreading_matrix(&profiles).unwrap();
        let engine = CompensationEngine::new(s);
        let true_m = array![[10.0_f32, 2.0]];
        let raw = engine.compensate(&true_m).unwrap();
        let m_arr = Array2::from_shape_vec(
            (2, 2),
            vec![
                m.values[0][0],
                m.values[0][1],
                m.values[1][0],
                m.values[1][1],
            ],
        )
        .unwrap();
        let comp = CompensationEngine::new(m_arr).compensate(&raw).unwrap();
        assert!((comp[[0, 0]] - 10.0).abs() < 1e-4);
        assert!((comp[[0, 1]] - 2.0).abs() < 1e-4);
    }

    #[test]
    fn positional_fallback_when_names_do_not_match() {
        let m = CompensationMatrix::from_csv("1,0.5\n0,1").unwrap();
        let data = array![[2.0_f32, 4.0]];
        let names = vec!["A".into(), "B".into()];
        let out = m.apply_to(&data, &names, &names).unwrap();
        assert!((out[[0, 0]] - 2.0).abs() < 1e-6);
        assert!((out[[0, 1]] - 5.0).abs() < 1e-6);
    }

    #[test]
    fn matrix_type_round_trips_through_serde() {
        let mut m = CompensationMatrix::from_csv(",FITC-A,PE-A\nFITC-A,1,0.1\nPE-A,0.2,1").unwrap();
        assert_eq!(m.matrix_type, MatrixType::Acquisition);
        m.matrix_type = MatrixType::Calculated;
        let json = serde_json::to_string(&m).unwrap();
        let back: CompensationMatrix = serde_json::from_str(&json).unwrap();
        assert_eq!(back.matrix_type, MatrixType::Calculated);
        let legacy = serde_json::json!({
            "channels": ["FITC-A", "PE-A"],
            "values": [[1.0, 0.1], [0.2, 1.0]],
        });
        let legacy_m: CompensationMatrix = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy_m.matrix_type, MatrixType::Acquisition);
    }

    #[test]
    fn validation_flags_finite_values_and_singular_matrices() {
        let bad_finite = CompensationMatrix::new(
            vec!["A".into(), "B".into()],
            vec![vec![f32::NAN, 0.1], vec![0.2, 1.0]],
        )
        .unwrap();
        let v = bad_finite.validate();
        assert!(!v.ok);
        assert!(!v.errors.is_empty());
        assert!(v.errors.iter().any(|e| e.contains("not finite")));

        let singular = CompensationMatrix::new(
            vec!["A".into(), "B".into()],
            vec![vec![1.0, 2.0], vec![2.0, 4.0]],
        )
        .unwrap();
        let v = singular.validate();
        assert!(!v.ok);
        assert!(v.errors.iter().any(|e| e.contains("singular")));
    }

    #[test]
    fn validation_warns_on_diagonal_drift() {
        let m = CompensationMatrix::new(
            vec!["A".into(), "B".into()],
            vec![vec![1.0, 0.1], vec![0.2, 0.95]],
        )
        .unwrap();
        let v = m.validate();
        assert!(v.ok);
        assert!(!v.warnings.is_empty());
        assert!(v.warnings[0].contains("diagonal"));
    }

    #[test]
    fn matrix_comparison_detects_differences() {
        let a = CompensationMatrix::from_csv(",FITC-A,PE-A\nFITC-A,1,0.1\nPE-A,0.2,1").unwrap();
        let same = a.clone();
        let edited =
            CompensationMatrix::from_csv(",FITC-A,PE-A\nFITC-A,1,0.2\nPE-A,0.2,1").unwrap();
        assert!(a.compare(&same).identical);
        assert_eq!(a.compare(&same).max_abs_difference, 0.0);
        let cmp = a.compare(&edited);
        assert!(!cmp.identical);
        assert!(cmp.max_abs_difference > 0.0);
        let renamed =
            CompensationMatrix::from_csv(",PE-A,FITC-A\nFITC-A,0.2,1\nPE-A,1,0.1").unwrap();
        assert!(!renamed.compare(&a).same_channels);
    }

    #[test]
    fn spreading_matrix_normalizes_singles_stain_profiles() {
        // Control A: channel A = 10, channel B = 2. Control B: channel A = 5, channel B = 40.
        let profiles = vec![vec![10.0, 2.0], vec![5.0, 40.0]];
        let m = spreading_matrix(&profiles).unwrap();
        // Diagonal entries are the normalized primaries = 1.
        assert!((m[[0, 0]] - 1.0).abs() < 1e-6);
        assert!((m[[1, 1]] - 1.0).abs() < 1e-6);
        // Off-diagonal: median of channel j in control i over the primary of control j.
        assert!((m[[0, 1]] - (2.0 / 40.0)).abs() < 1e-6);
        assert!((m[[1, 0]] - (5.0 / 10.0)).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_matches_hand_computed_fixture() {
        let a = [1.0, 2.0, 3.0];
        let b = [2.0, 4.0, 6.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
        let c = [2.0, -1.0, 0.0];
        assert!(cosine_similarity(&a, &c).abs() < 1e-6);
        assert_eq!(cosine_similarity(&[], &[1.0]), 0.0);
    }

    #[test]
    fn total_spread_sums_column_spreads() {
        let m = Array2::from_shape_vec((2, 2), vec![1.0, 0.05, 0.5, 1.0]).unwrap();
        // Column A: 1 + 0.5 = 1.5; column B: 0.05 + 1 = 1.05; total = 2.55.
        assert!((total_spread(&m) - 2.55).abs() < 1e-6);
    }

    #[test]
    fn solve_ols_recovers_exact_mixtures() {
        // sources[k] = [s_k1, s_k2, s_k3]; mixture = w0*s0 + w1*s1.
        let sources = vec![vec![1.0, 0.0, 1.0], vec![0.0, 1.0, 1.0]];
        let mixtures = vec![vec![1.0, 2.0, 3.0]]; // w = [1, 2]
        let weights = solve_ols(&sources, &mixtures).unwrap();
        assert_eq!(weights.len(), 1);
        assert!((weights[0][0] - 1.0).abs() < 1e-4);
        assert!((weights[0][1] - 2.0).abs() < 1e-4);
    }

    #[test]
    fn solve_ols_rejects_mismatched_lengths_and_empty_systems() {
        assert!(solve_ols(&[], &[]).is_err());
        assert!(solve_ols(&[vec![1.0, 2.0]], &[vec![1.0]]).is_err());
        assert!(solve_ols(&[vec![1.0, 2.0]], &[vec![1.0, 2.0, 3.0]]).is_err());
    }

    #[test]
    fn unmix_recovers_exact_weights_and_zero_residual() {
        let sources = vec![vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]];
        let mixtures = vec![vec![1.0, 2.0, 0.0]];
        let results = unmix(&sources, &mixtures).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mixture, 0);
        assert_eq!(results[0].weights, vec![1.0, 2.0]);
        assert_eq!(results[0].residual, 0.0);
    }

    #[test]
    fn unmix_reports_residual_for_noisy_mixture() {
        let sources = vec![vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]];
        let mixtures = vec![vec![1.0, 2.0, 0.7]];
        let results = unmix(&sources, &mixtures).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].weights, vec![1.0, 2.0]);
        assert!((results[0].residual - 0.7).abs() < 1e-4);
    }

    #[test]
    fn apply_group_compensates_every_member_and_preserves_order() {
        let engine = CompensationEngine::new(array![[1.0_f32, 0.5], [0.0, 1.0]]);
        let data = vec![array![[2.0, 4.0]], array![[3.0, 6.0]]];
        let out = apply_group(&engine, &data).unwrap();
        assert_eq!(out.len(), 2);
        // sample 0: [2, 4] -> [2*1 + 4*0, 2*0.5 + 4*1] = [2, 5]
        assert_eq!(out[0][[0, 0]], 2.0);
        assert_eq!(out[0][[0, 1]], 5.0);
        // sample 1: [3, 6] -> [3, 7.5]
        assert_eq!(out[1][[0, 0]], 3.0);
        assert_eq!(out[1][[0, 1]], 7.5);
    }

    #[test]
    fn spectral_weights_export_and_import_round_trip() {
        let results = vec![
            SpectralResult {
                mixture: 0,
                weights: vec![1.0, 2.0],
                residual: 0.0,
            },
            SpectralResult {
                mixture: 1,
                weights: vec![3.0, 4.0],
                residual: 0.1,
            },
        ];
        let csv = export_weights_csv(&results);
        let back = import_weights_csv(&csv).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0], vec![1.0, 2.0]);
        assert_eq!(back[1], vec![3.0, 4.0]);
    }

    #[test]
    fn import_weights_csv_rejects_non_numeric_rows() {
        assert!(import_weights_csv("1,2\nfoo,bar").is_err());
    }

    #[test]
    fn optimize_weights_recovers_exact_mixture_weights() {
        let sources = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 1.0],
        ];
        let mixtures = vec![vec![3.0, 6.0, 0.0]];
        let weights = optimize_weights(&sources, &mixtures, 25).unwrap();
        assert_eq!(weights.len(), 1);
        assert!((weights[0][0] - 3.0).abs() < 1e-3);
        assert!((weights[0][1] - 6.0).abs() < 1e-3);
        assert_eq!(weights[0][2], 0.0);
    }

    #[test]
    fn optimize_weights_is_deterministic() {
        let sources = vec![vec![1.0, 2.0], vec![2.0, 1.0]];
        let mixtures = vec![vec![3.0, 3.0], vec![1.0, 4.0]];
        let a = optimize_weights(&sources, &mixtures, 10).unwrap();
        let b = optimize_weights(&sources, &mixtures, 10).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn raw_overlay_keeps_raw_and_computed_compensated() {
        let engine = CompensationEngine::new(array![[1.0_f32, 0.0], [0.0, 1.0]]);
        let data = array![[2.0, 4.0]];
        let overlay = raw_overlay(&engine, &data).unwrap();
        assert_eq!(overlay.raw, data);
        assert_eq!(overlay.compensated, data);
    }

    #[test]
    fn animate_matrix_interpolates_between_endpoints() {
        let from = array![[1.0_f32, 2.0], [3.0, 4.0]];
        let to = array![[3.0, 4.0], [5.0, 6.0]];
        let frames = animate_matrix(&from, &to, 3);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0], from);
        assert_eq!(frames[2], to);
        assert_eq!(frames[1], array![[2.0, 3.0], [4.0, 5.0]]);
    }

    #[test]
    fn animate_matrix_single_frame_returns_source() {
        let from = array![[1.0_f32, 2.0], [3.0, 4.0]];
        let frames = animate_matrix(&from, &from, 1);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], from);
    }
}
