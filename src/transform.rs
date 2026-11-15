//! Axis transforms for flow-cytometry plots and statistics.
//!
//! Flow cytometry data commonly spans negative values, zero, and several
//! positive decades. A plain log axis cannot display that range. This module
//! provides the transform vocabulary used by plots, gates, and the GUI:
//!
//! * `Linear` and `Log`
//! * `Arcsinh`
//! * `Biexponential` (linear near zero, exponential tails)
//! * `Logicle` (a biexponential parameterisation)
//! * `Hyperlog` (arcsinh-based hyperlog approximation)
//! * `RobustLog` (signed log that accepts negative values)
//! * `Miltenyi` (arcsinh-family transform used by Miltenyi MQD files)
//!
//! All transforms expose a forward (`transform`) and inverse (`inverse`)
//! mapping. The biexponential family uses a numeric inverse; the closed-form
//! transforms use their analytic inverse.
//!
//! **Compatibility note:** FlowJo's exact proprietary parameter fitting is not
//! published. The formulas here follow the published transform families and are
//! covered by round-trip and monotonicity tests. The type is intentionally
//! serialisable so it can be replaced by a plugin transform later.

use serde::{Deserialize, Serialize};
use std::f64::consts::LN_10;

/// Parameters for the biexponential and logicle transforms.
///
/// `width_basis` controls the width of the approximately linear region around
/// zero. `positive_decades` / `negative_decades` control the exponential tails,
/// and `extra_negative_decades` extends the negative tail independently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BiexponentialParams {
    pub width_basis: f64,
    pub positive_decades: f64,
    pub negative_decades: f64,
    pub extra_negative_decades: f64,
    pub zero_channel: f64,
}

impl BiexponentialParams {
    pub const DEFAULT: Self = Self {
        width_basis: 1.0,
        positive_decades: 4.5,
        negative_decades: 1.0,
        extra_negative_decades: 0.0,
        zero_channel: 0.0,
    };

    pub const fn new() -> Self {
        Self::DEFAULT
    }

    /// Estimates a width basis from data.
    ///
    /// The 95th percentile of absolute values is used as a robust scale, with
    /// the width set to one tenth of that scale. This keeps the transformed
    /// range around ±10 for typical cytometry channels while avoiding overflow
    /// on channels with very large raw values.
    pub fn auto_width_basis(values: &[f64]) -> f64 {
        let mut magnitudes: Vec<f64> = values
            .iter()
            .copied()
            .filter(|v| v.is_finite() && *v != 0.0)
            .map(f64::abs)
            .collect();
        if magnitudes.is_empty() {
            return 1.0;
        }
        magnitudes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((magnitudes.len() as f64 - 1.0) * 0.95).round() as usize;
        let scale = magnitudes[idx.min(magnitudes.len() - 1)];
        (scale / 10.0).max(1e-9)
    }

    /// Returns a copy with `width_basis` fitted to the supplied values.
    pub fn with_auto_width_basis(mut self, values: &[f64]) -> Self {
        self.width_basis = Self::auto_width_basis(values);
        self
    }

    fn positive_slope(&self) -> f64 {
        LN_10 / self.positive_decades.max(1e-6)
    }

    fn negative_slope(&self) -> f64 {
        LN_10 / (self.negative_decades + self.extra_negative_decades).max(1e-6)
    }
}

impl Default for BiexponentialParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Parameters for the hyperlog approximation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HyperlogParams {
    pub width_basis: f64,
    pub decades: f64,
    pub zero_channel: f64,
}

impl HyperlogParams {
    pub const DEFAULT: Self = Self {
        width_basis: 1.0,
        decades: 4.5,
        zero_channel: 0.0,
    };
}

impl Default for HyperlogParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How a plot axis maps data values to positions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AxisScale {
    #[default]
    Linear,
    Log,
    Arcsinh {
        cofactor: f64,
    },
    Biexponential(BiexponentialParams),
    Logicle(BiexponentialParams),
    Hyperlog(HyperlogParams),
    RobustLog {
        scale: f64,
    },
    Miltenyi {
        scale: f64,
    },
    Lookup {
        x: Vec<f64>,
        y: Vec<f64>,
    },
}

impl AxisScale {
    /// All supported axis transforms, in display order.
    pub fn all() -> Vec<AxisScale> {
        vec![
            AxisScale::Linear,
            AxisScale::Log,
            AxisScale::Arcsinh { cofactor: 150.0 },
            AxisScale::Biexponential(BiexponentialParams::DEFAULT),
            AxisScale::Logicle(BiexponentialParams::DEFAULT),
            AxisScale::Hyperlog(HyperlogParams::DEFAULT),
            AxisScale::RobustLog { scale: 1.0 },
            AxisScale::Miltenyi { scale: 1.0 },
            AxisScale::Lookup {
                x: Vec::new(),
                y: Vec::new(),
            },
        ]
    }

    /// Forward transform: data value -> plot position.
    ///
    /// Returns `None` for non-finite values and for non-positive values on a
    /// log axis.
    pub fn transform(&self, v: f64) -> Option<f64> {
        if !v.is_finite() {
            return None;
        }
        match self {
            AxisScale::Linear => Some(v),
            AxisScale::Log => {
                if v > 0.0 {
                    Some(v.log10())
                } else {
                    None
                }
            }
            AxisScale::Arcsinh { cofactor } => Some((v / cofactor.max(1e-9)).asinh()),
            AxisScale::Biexponential(p) | AxisScale::Logicle(p) => Some(biex_forward(v, p)),
            AxisScale::Hyperlog(p) => {
                let scale = 10f64.powf(p.decades / 2.0);
                Some(((v - p.zero_channel) / p.width_basis.max(1e-12) * scale).asinh() / LN_10)
            }
            AxisScale::RobustLog { scale } => {
                let s = scale.abs().max(1e-12);
                Some(v.signum() * (1.0 + v.abs() / s).log10())
            }
            AxisScale::Miltenyi { scale } => Some((v / scale.abs().max(1e-9)).asinh() / LN_10),
            AxisScale::Lookup { x, y } => lookup_forward(x, y, v),
        }
    }

    /// Inverse transform: plot position -> data value.
    pub fn inverse(&self, t: f64) -> f64 {
        if !t.is_finite() {
            return f64::NAN;
        }
        match self {
            AxisScale::Linear => t,
            AxisScale::Log => 10f64.powf(t),
            AxisScale::Arcsinh { cofactor } => cofactor.max(1e-9) * t.sinh(),
            AxisScale::Biexponential(p) | AxisScale::Logicle(p) => biex_inverse(t, p),
            AxisScale::Hyperlog(p) => {
                let scale = 10f64.powf(p.decades / 2.0);
                p.zero_channel + p.width_basis.max(1e-12) * (t * LN_10).sinh() / scale
            }
            AxisScale::RobustLog { scale } => {
                let s = scale.abs().max(1e-12);
                t.signum() * s * (10f64.powf(t.abs()) - 1.0)
            }
            AxisScale::Miltenyi { scale } => scale.abs().max(1e-9) * (t * LN_10).sinh(),
            AxisScale::Lookup { x, y } => lookup_inverse(x, y, t),
        }
    }

    /// Human-readable label for this scale.
    pub fn label(&self) -> &'static str {
        match self {
            AxisScale::Linear => "Linear",
            AxisScale::Log => "Log",
            AxisScale::Arcsinh { .. } => "ArcSinh",
            AxisScale::Biexponential(_) => "Biexponential",
            AxisScale::Logicle(_) => "Logicle",
            AxisScale::Hyperlog(_) => "Hyperlog",
            AxisScale::RobustLog { .. } => "RobustLog",
            AxisScale::Miltenyi { .. } => "Miltenyi",
            AxisScale::Lookup { .. } => "Lookup",
        }
    }

    /// Whether this transform has a linear forward mapping.
    pub fn is_linear(&self) -> bool {
        matches!(self, AxisScale::Linear)
    }

    /// Whether this transform rejects non-positive values.
    pub fn positive_only(&self) -> bool {
        matches!(self, AxisScale::Log)
    }

    /// Returns a serialisable short key used by project files / CLI.
    pub fn key(&self) -> String {
        match self {
            AxisScale::Linear => "linear".to_string(),
            AxisScale::Log => "log".to_string(),
            AxisScale::Arcsinh { cofactor } => format!("arcsinh:{cofactor}"),
            AxisScale::Biexponential(p) => format!(
                "biex:{},{},{},{},{}",
                p.width_basis,
                p.positive_decades,
                p.negative_decades,
                p.extra_negative_decades,
                p.zero_channel
            ),
            AxisScale::Logicle(p) => format!(
                "logicle:{},{},{},{},{}",
                p.width_basis,
                p.positive_decades,
                p.negative_decades,
                p.extra_negative_decades,
                p.zero_channel
            ),
            AxisScale::Hyperlog(p) => {
                format!(
                    "hyperlog:{},{},{}",
                    p.width_basis, p.decades, p.zero_channel
                )
            }
            AxisScale::RobustLog { scale } => format!("robustlog:{scale}"),
            AxisScale::Miltenyi { scale } => format!("miltenyi:{scale}"),
            AxisScale::Lookup { x, y } => format!(
                "lookup:{}",
                x.iter()
                    .zip(y.iter())
                    .map(|(a, b)| format!("{a},{b}"))
                    .collect::<Vec<_>>()
                    .join(";")
            ),
        }
    }

    /// Parses a key produced by [`AxisScale::key`]. Unknown keys fall back to
    /// linear so older or hand-edited files remain loadable.
    pub fn from_key(key: &str) -> Self {
        let (kind, rest) = key.split_once(':').unwrap_or((key, ""));
        let values: Vec<f64> = rest
            .split(',')
            .filter_map(|s| s.trim().parse::<f64>().ok())
            .collect();
        match kind.trim().to_ascii_lowercase().as_str() {
            "log" => AxisScale::Log,
            "arcsinh" => AxisScale::Arcsinh {
                cofactor: values.first().copied().unwrap_or(150.0),
            },
            "biex" | "biexponential" => {
                let mut p = BiexponentialParams::DEFAULT;
                if let Some(v) = values.first() {
                    p.width_basis = *v;
                }
                if let Some(v) = values.get(1) {
                    p.positive_decades = *v;
                }
                if let Some(v) = values.get(2) {
                    p.negative_decades = *v;
                }
                if let Some(v) = values.get(3) {
                    p.extra_negative_decades = *v;
                }
                if let Some(v) = values.get(4) {
                    p.zero_channel = *v;
                }
                AxisScale::Biexponential(p)
            }
            "logicle" => {
                let mut p = BiexponentialParams::DEFAULT;
                if let Some(v) = values.first() {
                    p.width_basis = *v;
                }
                if let Some(v) = values.get(1) {
                    p.positive_decades = *v;
                }
                if let Some(v) = values.get(2) {
                    p.negative_decades = *v;
                }
                if let Some(v) = values.get(3) {
                    p.extra_negative_decades = *v;
                }
                if let Some(v) = values.get(4) {
                    p.zero_channel = *v;
                }
                AxisScale::Logicle(p)
            }
            "hyperlog" => {
                let mut p = HyperlogParams::DEFAULT;
                if let Some(v) = values.first() {
                    p.width_basis = *v;
                }
                if let Some(v) = values.get(1) {
                    p.decades = *v;
                }
                if let Some(v) = values.get(2) {
                    p.zero_channel = *v;
                }
                AxisScale::Hyperlog(p)
            }
            "robustlog" => AxisScale::RobustLog {
                scale: values.first().copied().unwrap_or(1.0),
            },
            "miltenyi" => AxisScale::Miltenyi {
                scale: values.first().copied().unwrap_or(1.0),
            },
            "lookup" => {
                let pairs: Vec<(f64, f64)> = rest
                    .split(';')
                    .filter_map(|pair| {
                        let mut it = pair.split(',');
                        let x = it.next()?.trim().parse::<f64>().ok()?;
                        let y = it.next()?.trim().parse::<f64>().ok()?;
                        Some((x, y))
                    })
                    .collect();
                let xs: Vec<f64> = pairs.iter().map(|(x, _)| *x).collect();
                let ys: Vec<f64> = pairs.iter().map(|(_, y)| *y).collect();
                AxisScale::Lookup { x: xs, y: ys }
            }
            _ => AxisScale::Linear,
        }
    }
}

/// Forward lookup: piecewise-linear interpolation on the supplied (x, y) pairs.
/// Returns `None` when the input is outside the supplied range.
fn lookup_forward(x: &[f64], y: &[f64], v: f64) -> Option<f64> {
    if x.len() != y.len() || x.is_empty() {
        return None;
    }
    let idx = x.partition_point(|&xv| xv <= v);
    if idx == 0 {
        return None;
    }
    let i = idx - 1;
    if i + 1 < x.len() {
        let t = (v - x[i]) / (x[i + 1] - x[i]);
        Some(y[i] + t * (y[i + 1] - y[i]))
    } else {
        Some(y[i])
    }
}

/// Inverse lookup: piecewise-linear inverse of [`lookup_forward`].
fn lookup_inverse(x: &[f64], y: &[f64], t: f64) -> f64 {
    if x.len() != y.len() || x.is_empty() {
        return f64::NAN;
    }
    if t < y[0] {
        return x[0];
    }
    if t > y[y.len() - 1] {
        return x[x.len() - 1];
    }
    let idx = y.partition_point(|&yv| yv < t);
    if idx == 0 {
        return x[0];
    }
    let i = idx - 1;
    let (y0, y1) = (y[i], y[i + 1]);
    let (x0, x1) = (x[i], x[i + 1]);
    if (y1 - y0).abs() < 1e-12 {
        return x0;
    }
    x0 + (t - y0) * (x1 - x0) / (y1 - y0)
}

fn biex_forward(v: f64, p: &BiexponentialParams) -> f64 {
    let width = p.width_basis.abs().max(1e-12);
    let x = (v - p.zero_channel) / width;
    biex_forward_normalized(x, p)
}

fn biex_forward_normalized(x: f64, p: &BiexponentialParams) -> f64 {
    let b = p.positive_slope();
    let d = p.negative_slope();
    let pos = (b * x).min(700.0).exp();
    let neg = (-d * x).min(700.0).exp();
    pos - neg
}

fn biex_inverse(t: f64, p: &BiexponentialParams) -> f64 {
    if t == 0.0 {
        return p.zero_channel;
    }
    let b = p.positive_slope();
    let d = p.negative_slope();
    // f(x) = exp(b*x) - exp(-d*x) is strictly increasing. Bracket the root
    // around zero, then bisect. The bracket is generous for extreme plot
    // coordinates while remaining finite.
    let (mut lo, mut hi) = if t > 0.0 {
        let hi = (t.ln() / b + 1.0).clamp(1.0, 1.0e6);
        (0.0, hi)
    } else {
        let lo = -(t.abs().ln() / d + 1.0).clamp(1.0, 1.0e6);
        (lo, 0.0)
    };
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        if biex_forward_normalized(mid, p) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let width = p.width_basis.abs().max(1e-12);
    p.zero_channel + ((lo + hi) / 2.0) * width
}

/// A validated monotone piecewise-linear lookup table.
///
/// Lookup transforms let a user paste a cytometer-specific calibration curve
/// (CSV: `raw,transformed` rows) and use it as an axis scale. Both columns must
/// be finite and strictly increasing so the transform is monotone and
/// invertible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LookupTable {
    x: Vec<f64>,
    y: Vec<f64>,
}

/// Why a lookup table was rejected.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LookupError {
    #[error("a lookup table needs at least two points")]
    TooFewPoints,
    #[error("x and y columns have different lengths ({x} vs {y})")]
    LengthMismatch { x: usize, y: usize },
    #[error("lookup value at row {row} is not finite")]
    NonFinite { row: usize },
    #[error("lookup x values must be strictly increasing (row {row})")]
    NotIncreasingX { row: usize },
    #[error("lookup y values must be strictly increasing (row {row})")]
    NotIncreasingY { row: usize },
    #[error("row {row} does not contain two numbers")]
    BadRow { row: usize },
}

impl LookupTable {
    /// Validate and construct a table from parallel x/y columns.
    pub fn new(x: Vec<f64>, y: Vec<f64>) -> Result<Self, LookupError> {
        if x.len() != y.len() {
            return Err(LookupError::LengthMismatch {
                x: x.len(),
                y: y.len(),
            });
        }
        if x.len() < 2 {
            return Err(LookupError::TooFewPoints);
        }
        for (row, (xv, yv)) in x.iter().zip(y.iter()).enumerate() {
            if !xv.is_finite() || !yv.is_finite() {
                return Err(LookupError::NonFinite { row });
            }
            if row > 0 && *xv <= x[row - 1] {
                return Err(LookupError::NotIncreasingX { row });
            }
            if row > 0 && *yv <= y[row - 1] {
                return Err(LookupError::NotIncreasingY { row });
            }
        }
        Ok(Self { x, y })
    }

    /// Parse a CSV table. A leading `x,y` header is optional; blank lines and
    /// `#` comments are ignored, and comma, semicolon, tab or whitespace
    /// separators are accepted.
    pub fn from_csv(text: &str) -> Result<Self, LookupError> {
        let mut x = Vec::new();
        let mut y = Vec::new();
        for (row, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line
                .split(|c: char| c == ',' || c == ';' || c == '\t' || c.is_whitespace())
                .filter(|f| !f.is_empty());
            let first = fields.next().ok_or(LookupError::BadRow { row })?;
            let second = fields.next().ok_or(LookupError::BadRow { row })?;
            // Skip a conventional header row.
            let fx = first.parse::<f64>();
            let fy = second.parse::<f64>();
            match (fx, fy) {
                (Ok(fx), Ok(fy)) => {
                    x.push(fx);
                    y.push(fy);
                }
                _ if row == 0 => continue,
                _ => return Err(LookupError::BadRow { row }),
            }
        }
        Self::new(x, y)
    }

    /// Serialize the table as a `raw,transformed` CSV with a header.
    pub fn to_csv(&self) -> String {
        let mut out = String::from("raw,transformed\n");
        for (xv, yv) in self.x.iter().zip(self.y.iter()) {
            out.push_str(&format!("{xv},{yv}\n"));
        }
        out
    }

    /// Read a table from a CSV file.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, std::io::Error> {
        let text = std::fs::read_to_string(path)?;
        Self::from_csv(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Write the table to a CSV file.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> Result<(), std::io::Error> {
        std::fs::write(path, self.to_csv())
    }

    pub fn x(&self) -> &[f64] {
        &self.x
    }

    pub fn y(&self) -> &[f64] {
        &self.y
    }

    /// Build the matching axis scale.
    pub fn to_scale(&self) -> AxisScale {
        AxisScale::Lookup {
            x: self.x.clone(),
            y: self.y.clone(),
        }
    }

    /// Configure a transform plugin from this table.
    pub fn to_plugin(&self, name: impl Into<String>) -> LookupPlugin {
        LookupPlugin {
            name: name.into(),
            table: self.clone(),
        }
    }
}

impl TryFrom<&AxisScale> for LookupTable {
    type Error = LookupError;

    fn try_from(scale: &AxisScale) -> Result<Self, Self::Error> {
        match scale {
            AxisScale::Lookup { x, y } => Self::new(x.clone(), y.clone()),
            _ => Err(LookupError::TooFewPoints),
        }
    }
}

/// A third-party (or user-defined) axis transform.
///
/// Implement this trait to plug a cytometer-specific user transform into
/// Flowers. The host keeps implementations behind a name, so they can be
/// looked up at runtime without changing [`AxisScale`].
pub trait CustomTransform: Send + Sync {
    /// Stable display/registry name.
    fn name(&self) -> &str;
    /// Forward mapping (`None` for values the transform rejects).
    fn forward(&self, value: f64) -> Option<f64>;
    /// Inverse mapping.
    fn inverse(&self, position: f64) -> f64;
}

/// A [`CustomTransform`] backed by a validated lookup table.
#[derive(Debug, Clone)]
pub struct LookupPlugin {
    name: String,
    table: LookupTable,
}

impl CustomTransform for LookupPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn forward(&self, value: f64) -> Option<f64> {
        self.table.to_scale().transform(value)
    }

    fn inverse(&self, position: f64) -> f64 {
        self.table.to_scale().inverse(position)
    }
}

/// Registry of custom transforms supplied by plugins.
#[derive(Default)]
pub struct TransformPluginHost {
    transforms: std::collections::HashMap<String, Box<dyn CustomTransform>>,
}

impl TransformPluginHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a transform. A transform with the same name is replaced.
    pub fn register(
        &mut self,
        transform: Box<dyn CustomTransform>,
    ) -> Option<Box<dyn CustomTransform>> {
        self.transforms
            .insert(transform.name().to_string(), transform)
    }

    /// Look up a registered transform by name.
    pub fn get(&self, name: &str) -> Option<&dyn CustomTransform> {
        self.transforms.get(name).map(|t| t.as_ref())
    }

    /// Registered names, sorted for stable UI ordering.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.transforms.keys().cloned().collect();
        names.sort();
        names
    }

    /// Apply a registered forward transform.
    pub fn forward(&self, name: &str, value: f64) -> Option<f64> {
        self.get(name)?.forward(value)
    }

    /// Apply a registered inverse transform.
    pub fn inverse(&self, name: &str, position: f64) -> Option<f64> {
        Some(self.get(name)?.inverse(position))
    }
}

impl std::fmt::Debug for TransformPluginHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransformPluginHost")
            .field("transforms", &self.names())
            .finish()
    }
}

/// Who a transform applies to.
///
/// Mirrors FlowJo's transform scoping: a transform can be global (the default
/// for a view), bound to a single parameter, applied to every compensated
/// parameter of a sample, or applied to every parameter of every sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TransformScope {
    /// Applies to the parameter it was explicitly set for; falls back to the
    /// view scale when nothing is registered.
    #[default]
    PerParameter,
    /// Applies to every compensated parameter of the sample.
    AllCompensated,
    /// Applies to every parameter of the sample.
    AllSamples,
    /// Applies to every parameter of every sample (global default).
    GlobalDefault,
}

impl TransformScope {
    /// Human-readable identifier used in keys and reports.
    pub fn as_str(&self) -> &'static str {
        match self {
            TransformScope::PerParameter => "per_parameter",
            TransformScope::AllCompensated => "all_compensated",
            TransformScope::AllSamples => "all_samples",
            TransformScope::GlobalDefault => "global_default",
        }
    }
}

/// A transform registry keyed by scope, sample and parameter.
///
/// Entries are `(scope, sample, param, scale)` tuples; a `None` parameter means
/// "every parameter within the scope" (used by the `AllSamples` and
/// `GlobalDefault` scopes). Lookups walk the most specific applicable entry
/// first so callers can compose scopes without nested maps.
#[derive(Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformRegistry {
    entries: Vec<(TransformScope, Option<usize>, Option<usize>, AxisScale)>,
}

impl TransformRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a per-parameter (or global, when the sample key is `None`) transform.
    ///
    /// This is the FlowJo `PerParameter` scope applied to a single parameter of a
    /// single sample (`sample = Some(i), param = Some(p)`) or of every sample
    /// (`sample = None, param = Some(p)`).
    pub fn set(&mut self, sample: Option<usize>, param: usize, scale: AxisScale) {
        self.set_scope(TransformScope::PerParameter, sample, Some(param), scale);
    }

    /// Set a transform under an explicit FlowJo scope.
    ///
    /// `sample = None` means "every sample"; `param = None` means "every
    /// parameter within the scope". Most specific entries win in [`Self::get`].
    pub fn set_scope(
        &mut self,
        scope: TransformScope,
        sample: Option<usize>,
        param: Option<usize>,
        scale: AxisScale,
    ) {
        self.entries
            .retain(|(s, smp, p, _)| !(*s == scope && *smp == sample && *p == param));
        self.entries.push((scope, sample, param, scale));
    }

    /// Global default: every parameter of every sample.
    pub fn set_global_default(&mut self, scale: AxisScale) {
        self.set_scope(TransformScope::GlobalDefault, None, None, scale);
    }

    /// Apply a transform to every parameter of one sample (`AllSamples`).
    pub fn set_all_samples(&mut self, sample: Option<usize>, scale: AxisScale) {
        self.set_scope(TransformScope::AllSamples, sample, None, scale);
    }

    /// Apply a transform to every compensated parameter of one sample
    /// (`AllCompensated`).
    pub fn set_all_compensated(&mut self, sample: Option<usize>, param: usize, scale: AxisScale) {
        self.set_scope(TransformScope::AllCompensated, sample, Some(param), scale);
    }

    /// Look up the effective transform for a sample/parameter pair, preferring
    /// the most specific applicable entry, in this order:
    ///
    /// 1. `PerParameter` override for the exact sample + parameter,
    /// 2. `AllCompensated` override for the exact sample + parameter,
    /// 3. `AllSamples` override for the exact sample (all parameters),
    /// 4. `PerParameter` global default for the parameter,
    /// 5. `AllCompensated` global default for the parameter,
    /// 6. `AllSamples` default (all samples, all parameters),
    /// 7. `GlobalDefault` (all samples, all parameters).
    ///
    /// Falls back to `None` (the caller then uses its view-scale default).
    pub fn get(&self, sample: Option<usize>, param: usize) -> Option<&AxisScale> {
        let per_sample_param = (TransformScope::PerParameter, sample, Some(param));
        if let Some(scale) = self.latest(&per_sample_param) {
            return Some(scale);
        }
        let compensated = (TransformScope::AllCompensated, sample, Some(param));
        if let Some(scale) = self.latest(&compensated) {
            return Some(scale);
        }
        let sample_wide = (TransformScope::AllSamples, sample, None);
        if let Some(scale) = self.latest(&sample_wide) {
            return Some(scale);
        }
        let global_param = (TransformScope::PerParameter, None, Some(param));
        if let Some(scale) = self.latest(&global_param) {
            return Some(scale);
        }
        let global_compensated = (TransformScope::AllCompensated, None, Some(param));
        if let Some(scale) = self.latest(&global_compensated) {
            return Some(scale);
        }
        let all_samples = (TransformScope::AllSamples, None, None);
        if let Some(scale) = self.latest(&all_samples) {
            return Some(scale);
        }
        let global_default = (TransformScope::GlobalDefault, None, None);
        if let Some(scale) = self.latest(&global_default) {
            return Some(scale);
        }
        None
    }

    fn latest(&self, key: &(TransformScope, Option<usize>, Option<usize>)) -> Option<&AxisScale> {
        self.entries
            .iter()
            .rev()
            .find(|(s, smp, p, _)| *s == key.0 && *smp == key.1 && *p == key.2)
            .map(|(_, _, _, scale)| scale)
    }

    /// Whether any parameter-specific override exists for `param`.
    pub fn has_override(&self, param: usize) -> bool {
        self.entries.iter().any(|(s, smp, p, _)| {
            smp.is_some() && *p == Some(param) && *s != TransformScope::GlobalDefault
        })
    }

    /// Seed per-parameter overrides for one sample (key `Some(sample)`), leaving
    /// global-default entries untouched.
    pub fn seed_sample(&mut self, sample: Option<usize>, overrides: &[(usize, AxisScale)]) {
        for (param, scale) in overrides {
            self.set(sample, *param, scale.clone());
        }
    }

    /// Clear every entry (reset).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Reset a single per-parameter override, returning whether it existed.
    ///
    /// After a reset the effective transform falls back to the registered global
    /// default (or the view scale) via [`Self::get`].
    pub fn reset(&mut self, sample: Option<usize>, param: usize) -> bool {
        self.reset_scope(TransformScope::PerParameter, sample, Some(param))
    }

    /// Reset an entry registered under an explicit scope, returning whether it
    /// existed.
    pub fn reset_scope(
        &mut self,
        scope: TransformScope,
        sample: Option<usize>,
        param: Option<usize>,
    ) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|(s, smp, p, _)| !(*s == scope && *smp == sample && *p == param));
        before > self.entries.len()
    }

    /// Number of registered overrides.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl std::fmt::Debug for TransformRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransformRegistry")
            .field("entries", &self.entries)
            .finish()
    }
}

/// How an automatic axis range is derived for a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AutorangeLimit {
    /// Explicit minimum and maximum (either may be `+inf` / `-inf`).
    MinMax { min: f64, max: f64 },
    /// Symmetric bounds around the p-th percentile of the observed values.
    Percentile { percentile: f64 },
    /// Symmetric percentile bounds derived from a fraction of the distribution.
    Percentage { fraction: f64 },
    /// Explicit custom minimum and maximum.
    CustomMinMax { min: f64, max: f64 },
}

impl Default for AutorangeLimit {
    fn default() -> Self {
        Self::MinMax { min: 0.0, max: 1.0 }
    }
}

impl AutorangeLimit {
    /// Human-readable limit description.
    pub fn label(&self) -> String {
        match self {
            AutorangeLimit::MinMax { min, max } => format!("min {min} / max {max}"),
            AutorangeLimit::CustomMinMax { min, max } => format!("custom min {min} / max {max}"),
            AutorangeLimit::Percentile { percentile } => format!("percentile {percentile}"),
            AutorangeLimit::Percentage { fraction } => format!("percentage {fraction}"),
        }
    }

    /// Resolve the (min, max) window for the observed `values`.
    pub fn resolve(&self, values: &[f64]) -> Option<(f64, f64)> {
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        if !min.is_finite() || !max.is_finite() || max <= min {
            return None;
        }
        match self {
            AutorangeLimit::MinMax { min, max } => Some((*min, *max)),
            AutorangeLimit::CustomMinMax { min, max } => Some((*min, *max)),
            AutorangeLimit::Percentile { percentile } => {
                let p = *percentile;
                if p < 0.0 || p > 50.0 {
                    return None;
                }
                let low = percentile_of(values, p);
                let high = percentile_of(values, 100.0 - p);
                Some((low, high))
            }
            AutorangeLimit::Percentage { fraction } => {
                let f = *fraction;
                if f <= 0.0 || f >= 1.0 {
                    return None;
                }
                let low = percentile_of(values, 100.0 * f);
                let high = percentile_of(values, 100.0 * (1.0 - f));
                Some((low, high))
            }
        }
    }
}

/// The p-th percentile of a finite sample (linear interpolation, p in `[0, 100]`).
pub fn percentile_of(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = values.iter().cloned().filter(|v| v.is_finite()).collect();
    if sorted.is_empty() {
        return 0.0;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len() - 1;
    let pos = (p / 100.0) * n as f64;
    let lower = pos.floor() as usize;
    let upper = (lower + 1).min(n);
    let frac = pos - lower as f64;
    sorted[lower] + (sorted[upper] - sorted[lower]) * frac
}

/// What an automatic range rule looks at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutorangeMatcher {
    /// Match on parameter name (exact or glob).
    Name(String),
    /// Match on parameter type / channel kind.
    ParameterType(String),
    /// Match on an FCS keyword (glob supported via `*`).
    Keyword(String),
}

impl AutorangeMatcher {
    /// Human-readable matcher description.
    pub fn matcher_label(&self) -> String {
        match self {
            AutorangeMatcher::Name(pattern) => format!("name ~ {pattern}"),
            AutorangeMatcher::ParameterType(kind) => format!("type ~ {kind}"),
            AutorangeMatcher::Keyword(pattern) => format!("keyword ~ {pattern}"),
        }
    }

    /// Does this matcher accept the given parameter?
    pub fn matches(
        &self,
        name: &str,
        kind: &str,
        keywords: &std::collections::HashMap<String, String>,
    ) -> bool {
        match self {
            AutorangeMatcher::Name(pattern) => {
                let base = pattern.trim_matches('*');
                if base.is_empty() {
                    return true;
                }
                let lower_name = name.to_ascii_lowercase();
                let lower_pattern = base.to_ascii_lowercase();
                lower_name.contains(&lower_pattern) || name.eq_ignore_ascii_case(base)
            }
            AutorangeMatcher::ParameterType(kind_pattern) => {
                kind.eq_ignore_ascii_case(kind_pattern)
            }
            AutorangeMatcher::Keyword(pattern) => {
                let base = pattern.trim_matches('*');
                if base.is_empty() {
                    return true;
                }
                let base_lower = base.to_ascii_lowercase();
                let name_lower = name.to_ascii_lowercase();
                name_lower.contains(&base_lower)
                    || keywords.iter().any(|(key, value)| {
                        let key_lower = key.to_ascii_lowercase();
                        let value_lower = value.to_ascii_lowercase();
                        (!key_lower.is_empty() && key_lower.contains(&base_lower))
                            || (!value_lower.is_empty() && value_lower.contains(&base_lower))
                    })
            }
        }
    }
}

/// An automatic axis-range rule derived from parameter properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutorangeRule {
    pub id: String,
    pub name: String,
    pub priority: u32,
    pub enabled: bool,
    pub matcher: AutorangeMatcher,
    pub limit: AutorangeLimit,
}

impl Default for AutorangeRule {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "Autorange".to_string(),
            priority: 0,
            enabled: true,
            matcher: AutorangeMatcher::Name("FSC".to_string()),
            limit: AutorangeLimit::default(),
        }
    }
}

impl AutorangeRule {
    /// Human-readable matcher description.
    pub fn matcher_label(&self) -> String {
        self.matcher.matcher_label()
    }

    /// Does this rule apply to the parameter?
    pub fn applies(
        &self,
        name: &str,
        kind: &str,
        keywords: &std::collections::HashMap<String, String>,
    ) -> bool {
        self.enabled && self.matcher.matches(name, kind, keywords)
    }

    /// Resolve the automatic (min, max) window for the observed values, if the
    /// rule's limit produces one.
    pub fn resolve(&self, values: &[f64]) -> Option<(f64, f64)> {
        self.limit.resolve(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_round_trip(scale: AxisScale, values: &[f64], tol: f64) {
        for &v in values {
            let t = scale.transform(v).expect("value should transform");
            let back = scale.inverse(t);
            assert!(
                (back - v).abs() <= tol * (1.0 + v.abs()),
                "{scale:?}: {v} -> {t} -> {back} (tol {tol})"
            );
        }
    }

    #[test]
    fn linear_and_log_round_trip() {
        assert_round_trip(AxisScale::Linear, &[-10.0, 0.0, 10.0, 1e6], 1e-12);
        assert_round_trip(AxisScale::Log, &[1e-3, 1.0, 10.0, 1e6], 1e-10);
    }

    #[test]
    fn closed_form_transforms_round_trip() {
        let values = [-5000.0, -100.0, 0.0, 100.0, 5000.0];
        assert_round_trip(AxisScale::Arcsinh { cofactor: 150.0 }, &values, 1e-10);
        assert_round_trip(AxisScale::RobustLog { scale: 10.0 }, &values, 1e-10);
        assert_round_trip(AxisScale::Miltenyi { scale: 10.0 }, &values, 1e-10);
        assert_round_trip(
            AxisScale::Hyperlog(HyperlogParams {
                width_basis: 100.0,
                decades: 4.5,
                zero_channel: 0.0,
            }),
            &values,
            1e-9,
        );
    }

    #[test]
    fn biexponential_and_logicle_round_trip_across_negative_values() {
        let p = BiexponentialParams {
            width_basis: 100.0,
            positive_decades: 4.5,
            negative_decades: 1.0,
            extra_negative_decades: 0.5,
            zero_channel: 0.0,
        };
        let values = [-5000.0, -1000.0, -100.0, 0.0, 100.0, 1000.0, 5000.0];
        assert_round_trip(AxisScale::Biexponential(p.clone()), &values, 1e-8);
        assert_round_trip(AxisScale::Logicle(p.clone()), &values, 1e-8);
    }

    #[test]
    fn biexponential_is_monotonic() {
        let p = BiexponentialParams {
            width_basis: 100.0,
            ..Default::default()
        };
        let scale = AxisScale::Biexponential(p);
        let mut previous = f64::NEG_INFINITY;
        for v in (-5000..=5000).step_by(100) {
            let t = scale.transform(v as f64).unwrap();
            assert!(t > previous, "not monotonic at {v}: {t} <= {previous}");
            previous = t;
        }
    }

    #[test]
    fn auto_width_basis_uses_robust_scale() {
        let values: Vec<f64> = (1..=100).map(|v| v as f64 * 1000.0).collect();
        let w = BiexponentialParams::auto_width_basis(&values);
        assert!(w > 0.0 && w.is_finite());
        // 95th percentile is near 95_000; width is one tenth of it.
        assert!(w > 5_000.0 && w < 12_000.0, "unexpected width {w}");
        assert_eq!(BiexponentialParams::auto_width_basis(&[]), 1.0);
    }

    #[test]
    fn transform_scope_covers_all_flowjo_scopes() {
        assert_eq!(TransformScope::PerParameter.as_str(), "per_parameter");
        assert_eq!(TransformScope::AllCompensated.as_str(), "all_compensated");
        assert_eq!(TransformScope::AllSamples.as_str(), "all_samples");
        assert_eq!(TransformScope::GlobalDefault.as_str(), "global_default");
        assert_eq!(TransformScope::default(), TransformScope::PerParameter);
    }

    #[test]
    fn transform_registry_resolves_most_specific_entry() {
        let mut registry = TransformRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.get(None, 0), None);

        // Global default for parameter 0.
        registry.set(None, 0, AxisScale::Log);
        assert_eq!(registry.get(None, 0), Some(&AxisScale::Log));

        // Per-sample override for sample 1, parameter 0.
        registry.set(Some(1), 0, AxisScale::Arcsinh { cofactor: 150.0 });
        assert_eq!(
            registry.get(Some(1), 0),
            Some(&AxisScale::Arcsinh { cofactor: 150.0 })
        );
        // Other samples fall back to the global default.
        assert_eq!(registry.get(Some(2), 0), Some(&AxisScale::Log));

        // A global default resolves directly but is not a parameter override.
        registry.set(None, 1, AxisScale::Linear);
        assert_eq!(registry.get(None, 1), Some(&AxisScale::Linear));
        assert!(!registry.has_override(1));

        // A per-sample override for parameter 2 counts as an override.
        registry.set(Some(0), 2, AxisScale::RobustLog { scale: 2.5 });
        assert!(registry.has_override(2));
        assert!(!registry.has_override(3));

        registry.clear();
        assert!(registry.is_empty());
    }

    #[test]
    fn registry_seeds_sample_overrides() {
        let mut registry = TransformRegistry::new();
        registry.set(None, 0, AxisScale::Log);
        registry.seed_sample(
            Some(2),
            &[
                (0, AxisScale::Arcsinh { cofactor: 150.0 }),
                (3, AxisScale::Linear),
            ],
        );
        assert_eq!(
            registry.get(Some(2), 0),
            Some(&AxisScale::Arcsinh { cofactor: 150.0 })
        );
        assert_eq!(registry.get(Some(2), 3), Some(&AxisScale::Linear));
        assert_eq!(registry.get(Some(3), 0), Some(&AxisScale::Log));
    }

    #[test]
    fn transform_registry_round_trips_through_serde() {
        let mut registry = TransformRegistry::new();
        registry.set(Some(0), 2, AxisScale::RobustLog { scale: 2.5 });
        let json = serde_json::to_string(&registry).unwrap();
        let back: TransformRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, registry);
    }

    #[test]
    fn registry_set_global_default_applies_to_all_samples_and_parameters() {
        let mut registry = TransformRegistry::new();
        assert_eq!(registry.get(Some(0), 0), None);
        registry.set_global_default(AxisScale::Log);
        assert_eq!(registry.get(Some(0), 0), Some(&AxisScale::Log));
        assert_eq!(registry.get(Some(0), 5), Some(&AxisScale::Log));
        assert_eq!(registry.get(None, 5), Some(&AxisScale::Log));
        assert!(!registry.has_override(0));
    }

    #[test]
    fn registry_all_samples_scope_covers_every_parameter_of_a_sample() {
        let mut registry = TransformRegistry::new();
        assert_eq!(registry.get(Some(0), 0), None);
        registry.set_all_samples(Some(2), AxisScale::Arcsinh { cofactor: 150.0 });
        assert_eq!(
            registry.get(Some(2), 0),
            Some(&AxisScale::Arcsinh { cofactor: 150.0 })
        );
        assert_eq!(
            registry.get(Some(2), 4),
            Some(&AxisScale::Arcsinh { cofactor: 150.0 })
        );
        // Other samples fall back to the global default.
        assert_eq!(registry.get(Some(3), 0), None);
        // A global all-samples default covers every sample and parameter.
        registry.set_all_samples(None, AxisScale::RobustLog { scale: 2.0 });
        assert_eq!(
            registry.get(Some(9), 7),
            Some(&AxisScale::RobustLog { scale: 2.0 })
        );
    }

    #[test]
    fn registry_all_compensated_scope_applies_to_the_compensated_parameter() {
        let mut registry = TransformRegistry::new();
        assert_eq!(registry.get(Some(1), 2), None);
        registry.set_all_compensated(Some(1), 2, AxisScale::RobustLog { scale: 3.0 });
        assert_eq!(
            registry.get(Some(1), 2),
            Some(&AxisScale::RobustLog { scale: 3.0 })
        );
        // Unrelated parameters keep their own overrides.
        registry.set(Some(1), 0, AxisScale::Linear);
        assert_eq!(registry.get(Some(1), 0), Some(&AxisScale::Linear));
        assert_eq!(registry.get(Some(0), 2), None);
    }

    #[test]
    fn registry_sample_wide_override_beats_global_default() {
        let mut registry = TransformRegistry::new();
        registry.set(None, 0, AxisScale::Log);
        registry.set_all_samples(Some(2), AxisScale::Arcsinh { cofactor: 150.0 });
        assert_eq!(
            registry.get(Some(2), 0),
            Some(&AxisScale::Arcsinh { cofactor: 150.0 })
        );
        assert_eq!(registry.get(Some(3), 0), Some(&AxisScale::Log));
    }

    #[test]
    fn registry_set_scope_replaces_previous_entry() {
        let mut registry = TransformRegistry::new();
        registry.set(Some(0), 1, AxisScale::Log);
        assert_eq!(registry.len(), 1);
        registry.set_scope(
            TransformScope::PerParameter,
            Some(0),
            Some(1),
            AxisScale::Arcsinh { cofactor: 150.0 },
        );
        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry.get(Some(0), 1),
            Some(&AxisScale::Arcsinh { cofactor: 150.0 })
        );
        let biex = AxisScale::Biexponential(BiexponentialParams::default());
        registry.set_scope(TransformScope::PerParameter, Some(0), Some(1), biex.clone());
        assert_eq!(registry.get(Some(0), 1), Some(&biex));
    }

    #[test]
    fn registry_reset_removes_override_and_falls_back_to_default() {
        let mut registry = TransformRegistry::new();
        assert!(!registry.reset(Some(0), 1));
        registry.set(Some(0), 1, AxisScale::Log);
        assert_eq!(registry.get(Some(0), 1), Some(&AxisScale::Log));
        assert!(registry.reset(Some(0), 1));
        assert_eq!(registry.get(Some(0), 1), None);
        assert!(!registry.reset(Some(0), 1));

        registry.set(None, 0, AxisScale::Arcsinh { cofactor: 150.0 });
        assert!(registry.reset(None, 0));
        assert_eq!(registry.get(None, 0), None);
    }

    #[test]
    fn registry_reset_scope_removes_scope_entry() {
        let mut registry = TransformRegistry::new();
        registry.set_all_samples(Some(2), AxisScale::Arcsinh { cofactor: 150.0 });
        assert!(registry.reset_scope(TransformScope::AllSamples, Some(2), None));
        assert_eq!(registry.get(Some(2), 0), None);
        assert!(!registry.reset_scope(TransformScope::AllSamples, Some(2), None));
    }

    #[test]
    fn key_round_trip() {
        for scale in AxisScale::all() {
            let parsed = AxisScale::from_key(&scale.key());
            assert_eq!(parsed, scale, "key {}", scale.key());
        }
        assert_eq!(AxisScale::from_key("log"), AxisScale::Log);
        assert_eq!(AxisScale::from_key("unknown"), AxisScale::Linear);
    }

    #[test]
    fn lookup_transform_round_trips_and_interpolates() {
        let scale = AxisScale::Lookup {
            x: vec![0.0, 10.0],
            y: vec![0.0, 100.0],
        };
        // Midpoint interpolates linearly.
        assert!((scale.transform(5.0).unwrap() - 50.0).abs() < 1e-9);
        assert!((scale.inverse(50.0) - 5.0).abs() < 1e-9);
        // Endpoints round trip.
        assert!((scale.inverse(scale.transform(10.0).unwrap()) - 10.0).abs() < 1e-9);
        // Outside the range -> None (forward), clamped (inverse).
        assert!(scale.transform(-1.0).is_none());
        assert!((scale.inverse(1000.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn autorange_min_max_limit_uses_explicit_bounds() {
        let values: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let rule = AutorangeRule {
            limit: AutorangeLimit::MinMax {
                min: 0.0,
                max: 10.0,
            },
            ..AutorangeRule::default()
        };
        assert_eq!(rule.resolve(&values), Some((0.0, 10.0)));
        let empty: Vec<f64> = Vec::new();
        assert_eq!(rule.resolve(&empty), None);
    }

    #[test]
    fn autorange_percentile_and_percentage_limits_are_symmetric() {
        let values: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let rule = AutorangeRule {
            limit: AutorangeLimit::Percentile { percentile: 5.0 },
            ..AutorangeRule::default()
        };
        let (lo, hi) = rule.resolve(&values).unwrap();
        assert!((lo - 4.95).abs() < 1e-9, "low={lo}");
        assert!((hi - 94.05).abs() < 1e-9, "high={hi}");

        let rule = AutorangeRule {
            limit: AutorangeLimit::Percentage { fraction: 0.05 },
            ..AutorangeRule::default()
        };
        let (lo, hi) = rule.resolve(&values).unwrap();
        assert!((lo - 4.95).abs() < 1e-9, "low={lo}");
        assert!((hi - 94.05).abs() < 1e-9, "high={hi}");

        // Out-of-range fractions resolve to None rather than nonsense bounds.
        let rule = AutorangeRule {
            limit: AutorangeLimit::Percentage { fraction: 0.0 },
            ..AutorangeRule::default()
        };
        assert_eq!(rule.resolve(&values), None);
    }

    #[test]
    fn autorange_matcher_matches_name_type_and_keyword() {
        let keywords = std::collections::HashMap::from([
            ("FIL".to_string(), "FSC".to_string()),
            ("DESC".to_string(), "Fluorescence".to_string()),
        ]);
        let by_name = AutorangeRule {
            matcher: AutorangeMatcher::Name("FSC*".to_string()),
            ..AutorangeRule::default()
        };
        assert!(by_name.applies("FSC-A", "Scatter", &keywords));
        assert!(!by_name.applies("SSC-A", "Scatter", &keywords));

        let by_type = AutorangeRule {
            matcher: AutorangeMatcher::ParameterType("Scatter".to_string()),
            ..AutorangeRule::default()
        };
        assert!(by_type.applies("FSC-A", "Scatter", &keywords));
        assert!(!by_type.applies("FSC-A", "Fluorescence", &keywords));

        let by_keyword = AutorangeRule {
            matcher: AutorangeMatcher::Keyword("FITC*".to_string()),
            ..AutorangeRule::default()
        };
        assert!(by_keyword.applies("FITC-A", "Fluorescence", &keywords));
        assert!(!by_keyword.applies("SSC-A", "Scatter", &keywords));
    }

    #[test]
    fn lookup_table_validates_and_round_trips_csv() {
        let table =
            LookupTable::from_csv("raw,transformed\n0,0\n10,100\n20,400\n# comment\n30,900\n")
                .unwrap();
        assert_eq!(table.x(), &[0.0, 10.0, 20.0, 30.0]);
        assert_eq!(table.y(), &[0.0, 100.0, 400.0, 900.0]);
        let csv = table.to_csv();
        assert!(csv.starts_with("raw,transformed"));
        assert_eq!(LookupTable::from_csv(&csv).unwrap(), table);

        // Without a header, whitespace/tab separators still work.
        let table = LookupTable::from_csv("0 0\n1\t2\n").unwrap();
        assert_eq!(table.y(), &[0.0, 2.0]);

        // The scale uses the table for forward/inverse interpolation.
        let scale = table.to_scale();
        assert!((scale.transform(1.0).unwrap() - 2.0).abs() < 1e-9);
        assert!((scale.inverse(2.0) - 1.0).abs() < 1e-9);
        assert!((scale.inverse(1.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn lookup_table_rejects_invalid_tables() {
        assert_eq!(
            LookupTable::new(vec![0.0], vec![0.0]),
            Err(LookupError::TooFewPoints)
        );
        assert_eq!(
            LookupTable::new(vec![0.0, 1.0], vec![0.0]),
            Err(LookupError::LengthMismatch { x: 2, y: 1 })
        );
        assert_eq!(
            LookupTable::new(vec![0.0, f64::NAN], vec![0.0, 1.0]),
            Err(LookupError::NonFinite { row: 1 })
        );
        assert_eq!(
            LookupTable::new(vec![0.0, 0.0], vec![0.0, 1.0]),
            Err(LookupError::NotIncreasingX { row: 1 })
        );
        assert_eq!(
            LookupTable::new(vec![0.0, 1.0], vec![1.0, 1.0]),
            Err(LookupError::NotIncreasingY { row: 1 })
        );
        assert!(matches!(
            LookupTable::from_csv("0,0\nbanana,1\n"),
            Err(LookupError::BadRow { row: 1 })
        ));
    }

    #[test]
    fn custom_transform_plugin_host_registers_and_applies() {
        let table = LookupTable::new(vec![0.0, 1.0, 2.0], vec![0.0, 10.0, 100.0]).unwrap();
        let plugin = table.to_plugin("calibration");
        assert_eq!(plugin.name(), "calibration");
        assert!((plugin.forward(0.5).unwrap() - 5.0).abs() < 1e-9);
        assert!((plugin.inverse(5.0) - 0.5).abs() < 1e-9);

        let mut host = TransformPluginHost::new();
        assert!(host.names().is_empty());
        assert!(host.register(Box::new(plugin)).is_none());
        assert_eq!(host.names(), ["calibration"]);
        assert!((host.forward("calibration", 1.5).unwrap() - 55.0).abs() < 1e-9);
        assert!((host.inverse("calibration", 55.0).unwrap() - 1.5).abs() < 1e-9);
        assert!(host.forward("missing", 1.0).is_none());
        assert!(host.inverse("missing", 1.0).is_none());
    }

    #[test]
    fn lookup_key_round_trip() {
        let scale = AxisScale::Lookup {
            x: vec![0.0, 10.0],
            y: vec![0.0, 100.0],
        };
        assert_eq!(AxisScale::from_key(&scale.key()), scale);
        let empty = AxisScale::Lookup {
            x: Vec::new(),
            y: Vec::new(),
        };
        assert_eq!(AxisScale::from_key(&empty.key()), empty);
    }
}
