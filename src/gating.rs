use ndarray::{ArrayView1, Axis};

/// A gate selects a subset of events from a flow cytometry sample.
///
/// Every gate implements `contains` (point-in-gate test) and inherits
/// `filter` (returns the indices of all passing events).
pub trait Gate: std::fmt::Debug + Send + Sync {
    /// Returns true if `event` is inside the gate.
    fn contains(&self, event: ArrayView1<f32>) -> bool;

    /// Returns the indices of events that pass the gate.
    fn filter(&self, data: &ndarray::Array2<f32>) -> Vec<usize> {
        let mut indices = Vec::new();
        for (idx, event) in data.axis_iter(Axis(0)).enumerate() {
            if self.contains(event) {
                indices.push(idx);
            }
        }
        indices
    }

    /// The parameter indices (channels) this gate operates on.
    fn parameter_indices(&self) -> Vec<usize>;
}

// ---------------------------------------------------------------------------
// Rectangular gate (axis-aligned box in 2D)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RectangularGate {
    pub param_idx_x: usize,
    pub param_idx_y: usize,
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
    /// Rotation of the rectangle about its centre, in radians.
    #[serde(default)]
    pub angle: f32,
}

impl RectangularGate {
    pub fn new(x: usize, y: usize, min_x: f32, max_x: f32, min_y: f32, max_y: f32) -> Self {
        Self {
            param_idx_x: x,
            param_idx_y: y,
            min_x,
            max_x,
            min_y,
            max_y,
            angle: 0.0,
        }
    }

    /// Sets a rotation (radians) about the rectangle's centre.
    pub fn with_angle(mut self, angle: f32) -> Self {
        self.angle = angle;
        self
    }
}

impl Gate for RectangularGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        let x = event[self.param_idx_x];
        let y = event[self.param_idx_y];
        if self.angle == 0.0 {
            return x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y;
        }
        let cx = (self.min_x + self.max_x) / 2.0;
        let cy = (self.min_y + self.max_y) / 2.0;
        let hw = (self.max_x - self.min_x) / 2.0;
        let hh = (self.max_y - self.min_y) / 2.0;
        let (s, c) = self.angle.sin_cos();
        let dx = x - cx;
        let dy = y - cy;
        // Rotate the point into the rectangle's local frame.
        let lx = dx * c + dy * s;
        let ly = -dx * s + dy * c;
        lx.abs() <= hw && ly.abs() <= hh
    }
    fn parameter_indices(&self) -> Vec<usize> {
        vec![self.param_idx_x, self.param_idx_y]
    }
}

// ---------------------------------------------------------------------------
// Ellipse gate (2D)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EllipseGate {
    pub param_idx_x: usize,
    pub param_idx_y: usize,
    pub center_x: f32,
    pub center_y: f32,
    pub radius_x: f32,
    pub radius_y: f32,
    /// Rotation of the ellipse about its centre, in radians.
    #[serde(default)]
    pub angle: f32,
}

impl EllipseGate {
    pub fn new(
        x: usize,
        y: usize,
        center_x: f32,
        center_y: f32,
        radius_x: f32,
        radius_y: f32,
    ) -> Self {
        Self {
            param_idx_x: x,
            param_idx_y: y,
            center_x,
            center_y,
            radius_x,
            radius_y,
            angle: 0.0,
        }
    }

    /// Sets a rotation (radians) about the ellipse's centre.
    pub fn with_angle(mut self, angle: f32) -> Self {
        self.angle = angle;
        self
    }
}

impl Gate for EllipseGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        let x = event[self.param_idx_x];
        let y = event[self.param_idx_y];
        let dx = x - self.center_x;
        let dy = y - self.center_y;
        let (s, c) = self.angle.sin_cos();
        let lx = dx * c + dy * s;
        let ly = -dx * s + dy * c;
        let ex = lx / self.radius_x;
        let ey = ly / self.radius_y;
        (ex * ex) + (ey * ey) <= 1.0
    }
    fn parameter_indices(&self) -> Vec<usize> {
        vec![self.param_idx_x, self.param_idx_y]
    }
}

// ---------------------------------------------------------------------------
// Polygon gate (arbitrary closed polygon in 2D, even-odd rule)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PolygonGate {
    pub param_idx_x: usize,
    pub param_idx_y: usize,
    /// Vertices of the polygon in order.
    pub vertices: Vec<(f32, f32)>,
}

impl PolygonGate {
    pub fn new(x: usize, y: usize, vertices: Vec<(f32, f32)>) -> Self {
        Self {
            param_idx_x: x,
            param_idx_y: y,
            vertices,
        }
    }
}

impl Gate for PolygonGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        let x = event[self.param_idx_x];
        let y = event[self.param_idx_y];
        let verts = &self.vertices;
        if verts.len() < 3 {
            return false;
        }
        let mut inside = false;
        let mut j = verts.len() - 1;
        for i in 0..verts.len() {
            let (xi, yi) = verts[i];
            let (xj, yj) = verts[j];
            if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
                inside = !inside;
            }
            j = i;
        }
        inside
    }
    fn parameter_indices(&self) -> Vec<usize> {
        vec![self.param_idx_x, self.param_idx_y]
    }
}

// ---------------------------------------------------------------------------
// Quadrant gate (four quadrants defined by a vertical and horizontal line)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Quadrant {
    UpperLeft,
    UpperRight,
    LowerLeft,
    LowerRight,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuadrantGate {
    pub param_idx_x: usize,
    pub param_idx_y: usize,
    pub x: f32,
    pub y: f32,
    /// Which of the four quadrants to select.
    pub quadrant: Quadrant,
}

impl QuadrantGate {
    pub fn new(x: usize, y: usize, x_split: f32, y_split: f32, quadrant: Quadrant) -> Self {
        Self {
            param_idx_x: x,
            param_idx_y: y,
            x: x_split,
            y: y_split,
            quadrant,
        }
    }

    /// Returns all four quadrants as separate gates.
    pub fn all(x: usize, y: usize, x_split: f32, y_split: f32) -> [Self; 4] {
        [
            Self::new(x, y, x_split, y_split, Quadrant::UpperLeft),
            Self::new(x, y, x_split, y_split, Quadrant::UpperRight),
            Self::new(x, y, x_split, y_split, Quadrant::LowerLeft),
            Self::new(x, y, x_split, y_split, Quadrant::LowerRight),
        ]
    }
}

impl Gate for QuadrantGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        let xv = event[self.param_idx_x];
        let yv = event[self.param_idx_y];
        match self.quadrant {
            Quadrant::UpperLeft => xv <= self.x && yv >= self.y,
            Quadrant::UpperRight => xv >= self.x && yv >= self.y,
            Quadrant::LowerLeft => xv <= self.x && yv <= self.y,
            Quadrant::LowerRight => xv >= self.x && yv <= self.y,
        }
    }
    fn parameter_indices(&self) -> Vec<usize> {
        vec![self.param_idx_x, self.param_idx_y]
    }
}

// ---------------------------------------------------------------------------
// Bilinear gate (two crossing lines split the plane into four wedges)
// ---------------------------------------------------------------------------

/// A bilinear gate: two straight lines crossing at a common center divide the
/// plot into four wedge-shaped regions. A `BilinearGate` selects one of those
/// regions, identified by which side of each line it lies on.
///
/// The signed distance of a point `p` from the line through the center with
/// orientation `angle` is `d = (p.x - cx) * sin(angle) - (p.y - cy) * cos(angle)`.
/// `side` is `true` when the point is on the positive side (`d >= 0`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BilinearGate {
    pub param_idx_x: usize,
    pub param_idx_y: usize,
    /// Intersection point of the two dividing lines.
    pub center_x: f32,
    pub center_y: f32,
    /// Orientation (radians) of the first dividing line.
    pub angle1: f32,
    /// Orientation (radians) of the second dividing line.
    pub angle2: f32,
    /// Half-length of each drawn line (a rendering aid; does not affect `contains`).
    pub radius: f32,
    /// Which side of line 1 to include (`true` => positive side).
    pub side1: bool,
    /// Which side of line 2 to include (`true` => positive side).
    pub side2: bool,
}

impl BilinearGate {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        x: usize,
        y: usize,
        center_x: f32,
        center_y: f32,
        angle1: f32,
        angle2: f32,
        radius: f32,
        side1: bool,
        side2: bool,
    ) -> Self {
        Self {
            param_idx_x: x,
            param_idx_y: y,
            center_x,
            center_y,
            angle1,
            angle2,
            radius,
            side1,
            side2,
        }
    }

    /// Returns all four wedge regions as separate gates.
    ///
    /// Order: (+,+), (+,-), (-,+), (-,-) — i.e. side1/side2 combinations.
    pub fn all(
        x: usize,
        y: usize,
        center_x: f32,
        center_y: f32,
        angle1: f32,
        angle2: f32,
        radius: f32,
    ) -> [Self; 4] {
        [
            Self::new(x, y, center_x, center_y, angle1, angle2, radius, true, true),
            Self::new(
                x, y, center_x, center_y, angle1, angle2, radius, true, false,
            ),
            Self::new(
                x, y, center_x, center_y, angle1, angle2, radius, false, true,
            ),
            Self::new(
                x, y, center_x, center_y, angle1, angle2, radius, false, false,
            ),
        ]
    }

    fn signed_side(x: f32, y: f32, cx: f32, cy: f32, angle: f32) -> f32 {
        (x - cx) * angle.sin() - (y - cy) * angle.cos()
    }
}

impl Gate for BilinearGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        let xv = event[self.param_idx_x];
        let yv = event[self.param_idx_y];
        let d1 = Self::signed_side(xv, yv, self.center_x, self.center_y, self.angle1);
        let d2 = Self::signed_side(xv, yv, self.center_x, self.center_y, self.angle2);
        (d1 >= 0.0) == self.side1 && (d2 >= 0.0) == self.side2
    }
    fn parameter_indices(&self) -> Vec<usize> {
        vec![self.param_idx_x, self.param_idx_y]
    }
}

// ---------------------------------------------------------------------------
// Line gate (one side of a line segment, restricted to its horizontal extent)
// ---------------------------------------------------------------------------

/// A line gate selects the events on one side of a straight line segment,
/// restricted to the segment's horizontal extent. This is the "region under the
/// line" shape used by the GUI's bilinear/line gate: a point is inside when its
/// X lies between the segment's endpoints and its Y is below (or above) the
/// segment at that X.
///
/// The gate is unbounded in the Y direction, so it stays meaningful no matter
/// how the plot window is later zoomed or panned. Renderers clip the visible
/// fill to the current axis range, which is what makes it look like a
/// full-height rectangle on screen.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LineGate {
    pub param_idx_x: usize,
    pub param_idx_y: usize,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    /// When true, events below the line (`y <= line_y(x)`) are selected;
    /// when false, events above the line are selected.
    pub below: bool,
}

impl LineGate {
    pub fn new(x: usize, y: usize, x1: f32, y1: f32, x2: f32, y2: f32, below: bool) -> Self {
        Self {
            param_idx_x: x,
            param_idx_y: y,
            x1,
            y1,
            x2,
            y2,
            below,
        }
    }

    /// Y value of the line at `x` (works for any non-vertical orientation).
    fn line_y(&self, x: f32) -> f32 {
        let dx = self.x2 - self.x1;
        if dx.abs() < 1e-9 {
            (self.y1 + self.y2) / 2.0
        } else {
            self.y1 + (self.y2 - self.y1) * (x - self.x1) / dx
        }
    }
}

impl Gate for LineGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        let x = event[self.param_idx_x];
        let y = event[self.param_idx_y];
        let xa = self.x1.min(self.x2);
        let xb = self.x1.max(self.x2);
        // A (near-)vertical segment has no meaningful X extent; fall back to an
        // unbounded half-plane threshold at the segment's midpoint.
        if (xb - xa).abs() > 1e-9 && (x < xa || x > xb) {
            return false;
        }
        let line_y = self.line_y(x);
        if self.below { y <= line_y } else { y >= line_y }
    }
    fn parameter_indices(&self) -> Vec<usize> {
        vec![self.param_idx_x, self.param_idx_y]
    }
}

// ---------------------------------------------------------------------------
// 1D range gate
// ---------------------------------------------------------------------------

/// A one-dimensional interval on a single parameter.
///
/// Range gates are the natural gate for histograms and for thresholds such as
/// "CD4 positive". They are inclusive at both ends by default; FlowJo-style
/// exclusive bounds are available through [`RangeGate::with_bounds`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RangeGate {
    pub param_idx: usize,
    pub low: f32,
    pub high: f32,
    pub include_low: bool,
    pub include_high: bool,
}

impl RangeGate {
    pub fn new(param_idx: usize, low: f32, high: f32) -> Self {
        Self {
            param_idx,
            low: low.min(high),
            high: low.max(high),
            include_low: true,
            include_high: true,
        }
    }

    pub fn with_bounds(mut self, include_low: bool, include_high: bool) -> Self {
        self.include_low = include_low;
        self.include_high = include_high;
        self
    }
}

impl Gate for RangeGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        let v = event[self.param_idx];
        let above = if self.include_low {
            v >= self.low
        } else {
            v > self.low
        };
        let below = if self.include_high {
            v <= self.high
        } else {
            v < self.high
        };
        above && below
    }

    fn parameter_indices(&self) -> Vec<usize> {
        vec![self.param_idx]
    }
}

// ---------------------------------------------------------------------------
// Time / timeslice gate
// ---------------------------------------------------------------------------

/// How a [`TimeGate`] interprets its bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TimeGateMode {
    /// Bounds are values of the time parameter (seconds, milliseconds, etc.).
    Value,
    /// Bounds are zero-based event indices (acquisition order).
    EventIndex,
}

/// A gate over the acquisition time parameter or over acquisition order.
///
/// FlowJo's time gates are used for kinetics and for selecting a stable slice
/// of acquisition. In `EventIndex` mode the gate does not depend on any event
/// value, so [`Gate::filter`] is overridden to use the event's row index.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TimeGate {
    pub param_idx: usize,
    pub start: f32,
    pub end: f32,
    pub mode: TimeGateMode,
}

impl TimeGate {
    pub fn new(param_idx: usize, start: f32, end: f32) -> Self {
        Self {
            param_idx,
            start: start.min(end),
            end: start.max(end),
            mode: TimeGateMode::Value,
        }
    }

    pub fn with_mode(mut self, mode: TimeGateMode) -> Self {
        self.mode = mode;
        self
    }
}

impl Gate for TimeGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        match self.mode {
            TimeGateMode::Value => {
                let v = event[self.param_idx];
                v >= self.start && v <= self.end
            }
            // Without an event index the value-only trait method cannot decide.
            // `filter` below implements acquisition-order selection.
            TimeGateMode::EventIndex => false,
        }
    }

    fn filter(&self, data: &ndarray::Array2<f32>) -> Vec<usize> {
        match self.mode {
            TimeGateMode::Value => {
                let mut indices = Vec::new();
                for (idx, event) in data.axis_iter(Axis(0)).enumerate() {
                    let v = event[self.param_idx];
                    if v >= self.start && v <= self.end {
                        indices.push(idx);
                    }
                }
                indices
            }
            TimeGateMode::EventIndex => {
                let start = self.start.max(0.0).floor() as usize;
                let end = self.end.max(0.0).ceil() as usize;
                (start..end.min(data.nrows())).collect()
            }
        }
    }

    fn parameter_indices(&self) -> Vec<usize> {
        vec![self.param_idx]
    }
}

// ---------------------------------------------------------------------------
// Mask gate (explicit event membership, standalone or imported from a file)
// ---------------------------------------------------------------------------

/// A gate that selects events by row index rather than by parameter values.
///
/// FlowJo "mask" gates let you pick events explicitly (for example after
/// reviewing a plot, or by importing a mask computed elsewhere). Indices are
/// stored sorted and de-duplicated, so membership tests and serialization are
/// deterministic regardless of insertion order.
///
/// Because membership is index-based, [`Gate::contains`] cannot decide from an
/// event's values alone and therefore returns `false`. Use [`Gate::filter`],
/// which applies the indices directly to the data matrix and skips indices
/// beyond the end of the data.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MaskGate {
    /// Sorted, de-duplicated zero-based event indices.
    indices: Vec<usize>,
}

impl MaskGate {
    /// Create a mask from event indices. The input is sorted and de-duplicated.
    pub fn new<I: IntoIterator<Item = usize>>(indices: I) -> Self {
        let mut indices: Vec<usize> = indices.into_iter().collect();
        indices.sort_unstable();
        indices.dedup();
        Self { indices }
    }

    /// Build a mask from a packed bitset (one bit per event, LSB first).
    pub fn from_bits(bits: &[u8]) -> Self {
        let mut indices = Vec::new();
        for (byte_idx, byte) in bits.iter().enumerate() {
            for bit in 0..8 {
                if byte & (1 << bit) != 0 {
                    indices.push(byte_idx * 8 + bit);
                }
            }
        }
        Self::new(indices)
    }

    /// Pack the mask into a bitset. Bits beyond the last selected index are
    /// zero, and the byte length covers every selected index.
    pub fn to_bits(&self) -> Vec<u8> {
        let Some(&last) = self.indices.last() else {
            return Vec::new();
        };
        let mut bits = vec![0u8; last / 8 + 1];
        for &index in &self.indices {
            bits[index / 8] |= 1 << (index % 8);
        }
        bits
    }

    /// The selected event indices (ascending).
    pub fn indices(&self) -> &[usize] {
        &self.indices
    }

    /// True when `index` is in the mask.
    pub fn contains_index(&self, index: usize) -> bool {
        self.indices.binary_search(&index).is_ok()
    }

    /// Indices at or beyond `event_count`, i.e. that cannot refer to a real
    /// event. Useful for validating an imported mask before applying it.
    pub fn out_of_range(&self, event_count: usize) -> Vec<usize> {
        self.indices
            .iter()
            .copied()
            .filter(|&i| i >= event_count)
            .collect()
    }

    /// Number of selected events.
    pub fn len(&self) -> usize {
        self.indices.len()
    }

    /// True when the mask selects no events.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

impl Gate for MaskGate {
    fn contains(&self, _event: ArrayView1<f32>) -> bool {
        // Membership depends on row order, not values; see `filter`.
        false
    }

    fn filter(&self, data: &ndarray::Array2<f32>) -> Vec<usize> {
        let event_count = data.nrows();
        self.indices
            .iter()
            .copied()
            .filter(|&i| i < event_count)
            .collect()
    }

    fn parameter_indices(&self) -> Vec<usize> {
        // A mask is not defined on any parameter.
        Vec::new()
    }
}

/// On-disk layout of a mask file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MaskFileFormat {
    /// One bit per event, least-significant bit first, packed into bytes.
    Bits,
    /// Decimal event indices separated by whitespace, commas or semicolons.
    /// `#` starts a comment that runs to the end of the line.
    Indices,
}

/// Errors produced while importing a mask file.
#[derive(Debug, thiserror::Error)]
pub enum MaskError {
    #[error("failed to read mask file '{}': {source}", path.display())]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid event index '{token}' on line {line} of mask file: {reason}")]
    InvalidIndex {
        token: String,
        /// 1-based source line.
        line: usize,
        reason: String,
    },
}

/// A [`MaskGate`] imported from an external mask file.
///
/// The parsed membership is embedded in the gate, so evaluation never depends
/// on the file still being present after import. `path` and `format` are kept
/// for provenance and re-import.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileMaskGate {
    /// Source file the mask was imported from.
    pub path: std::path::PathBuf,
    /// Format that was used to parse the file.
    pub format: MaskFileFormat,
    /// Parsed membership, evaluated without touching the filesystem.
    pub mask: MaskGate,
}

impl FileMaskGate {
    /// Parse an indices-format mask from text, recording `path` as its origin.
    ///
    /// Tokens may be separated by whitespace, commas or semicolons; `#` starts
    /// a comment. Duplicate indices are collapsed.
    pub fn from_text(path: impl Into<std::path::PathBuf>, text: &str) -> Result<Self, MaskError> {
        Ok(Self {
            path: path.into(),
            format: MaskFileFormat::Indices,
            mask: Self::parse_indices(text)?,
        })
    }

    /// Read a mask file and parse it according to `format`.
    pub fn load(
        path: impl Into<std::path::PathBuf>,
        format: MaskFileFormat,
    ) -> Result<Self, MaskError> {
        let path = path.into();
        let bytes = std::fs::read(&path).map_err(|source| MaskError::Io {
            path: path.clone(),
            source,
        })?;
        let mask = match format {
            MaskFileFormat::Bits => MaskGate::from_bits(&bytes),
            MaskFileFormat::Indices => {
                let text = String::from_utf8_lossy(&bytes);
                Self::parse_indices(&text)?
            }
        };
        Ok(Self { path, format, mask })
    }

    fn parse_indices(text: &str) -> Result<MaskGate, MaskError> {
        let mut indices = Vec::new();
        for (line_idx, raw_line) in text.lines().enumerate() {
            let line = raw_line.split('#').next().unwrap_or("");
            for token in line.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
                let token = token.trim();
                if token.is_empty() {
                    continue;
                }
                let index: usize = token.parse().map_err(|e: std::num::ParseIntError| {
                    MaskError::InvalidIndex {
                        token: token.to_string(),
                        line: line_idx + 1,
                        reason: e.to_string(),
                    }
                })?;
                indices.push(index);
            }
        }
        Ok(MaskGate::new(indices))
    }

    /// The selected event indices (ascending).
    pub fn indices(&self) -> &[usize] {
        self.mask.indices()
    }
}

impl Gate for FileMaskGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        self.mask.contains(event)
    }

    fn filter(&self, data: &ndarray::Array2<f32>) -> Vec<usize> {
        self.mask.filter(data)
    }

    fn parameter_indices(&self) -> Vec<usize> {
        self.mask.parameter_indices()
    }
}

// ---------------------------------------------------------------------------
// Tethered range gate (bounds follow a statistic of a reference population)
// ---------------------------------------------------------------------------

/// Format a percentile value with an ordinal suffix (1st, 2nd, 3rd, ...).
fn percentile_ordinal(p: f64) -> String {
    let n = p.round() as i64;
    let suffix = if (11..=13).contains(&(n % 100)) {
        "th"
    } else {
        match n % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    format!("{n}{suffix}")
}

/// Statistic used to place a [`TetheredRangeGate`]'s bounds.
///
/// The statistic is computed over the reference population's values for the
/// gate's parameter; the resulting value is then transformed by the gate's
/// [`TetherBound`]s into the actual low/high limits.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TetherStatistic {
    Mean,
    Median,
    /// Most frequent value (falls back to the densest region).
    Mode,
    /// Arbitrary percentile in `[0, 100]`.
    Percentile(f64),
    Percent5,
    Percent10,
    Percent90,
    Percent95,
    Min,
    Max,
    StdDev,
    Cv,
}

impl TetherStatistic {
    /// Human-readable label for menus and gate summaries.
    pub fn label(&self) -> String {
        match self {
            TetherStatistic::Mean => "mean".to_string(),
            TetherStatistic::Median => "median".to_string(),
            TetherStatistic::Mode => "mode".to_string(),
            TetherStatistic::Percentile(p) => {
                format!("{} percentile", percentile_ordinal(*p))
            }
            TetherStatistic::Percent5 => "5th percentile".to_string(),
            TetherStatistic::Percent10 => "10th percentile".to_string(),
            TetherStatistic::Percent90 => "90th percentile".to_string(),
            TetherStatistic::Percent95 => "95th percentile".to_string(),
            TetherStatistic::Min => "minimum".to_string(),
            TetherStatistic::Max => "maximum".to_string(),
            TetherStatistic::StdDev => "SD".to_string(),
            TetherStatistic::Cv => "CV".to_string(),
        }
    }

    /// Compute the statistic from an already-sorted finite slice.
    fn compute_from_sorted(&self, sorted: &[f64]) -> f64 {
        if sorted.is_empty() {
            return f64::NAN;
        }
        let mean = || sorted.iter().sum::<f64>() / sorted.len() as f64;
        match *self {
            TetherStatistic::Mean => mean(),
            TetherStatistic::Median => crate::statistics::percentile_sorted(sorted, 50.0),
            TetherStatistic::Mode => crate::statistics::mode(sorted),
            TetherStatistic::Percentile(p) => crate::statistics::percentile_sorted(sorted, p),
            TetherStatistic::Percent5 => crate::statistics::percentile_sorted(sorted, 5.0),
            TetherStatistic::Percent10 => crate::statistics::percentile_sorted(sorted, 10.0),
            TetherStatistic::Percent90 => crate::statistics::percentile_sorted(sorted, 90.0),
            TetherStatistic::Percent95 => crate::statistics::percentile_sorted(sorted, 95.0),
            TetherStatistic::Min => sorted[0],
            TetherStatistic::Max => sorted[sorted.len() - 1],
            TetherStatistic::StdDev => {
                let m = mean();
                (sorted.iter().map(|v| (v - m).powi(2)).sum::<f64>() / sorted.len() as f64).sqrt()
            }
            TetherStatistic::Cv => {
                let m = mean();
                if m == 0.0 {
                    f64::NAN
                } else {
                    let sd = (sorted.iter().map(|v| (v - m).powi(2)).sum::<f64>()
                        / sorted.len() as f64)
                        .sqrt();
                    sd / m.abs()
                }
            }
        }
    }
}

/// One end of a tethered range: `statistic * factor + offset`.
///
/// Offsets move a bound by a fixed amount; factors scale it proportionally.
/// Combining both supports FlowJo-style "median ± 2 SD" or "mean × 0.8"
/// tethers.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TetherBound {
    pub factor: f64,
    pub offset: f64,
}

impl TetherBound {
    /// A bound that is the statistic plus a fixed offset.
    pub fn offset(offset: f64) -> Self {
        Self {
            factor: 1.0,
            offset,
        }
    }

    /// A bound that is the statistic scaled by a factor.
    pub fn factor(factor: f64) -> Self {
        Self {
            factor,
            offset: 0.0,
        }
    }

    /// A scaled, offset bound: `statistic * factor + offset`.
    pub fn new(factor: f64, offset: f64) -> Self {
        Self { factor, offset }
    }

    /// Resolve this bound against a statistic value.
    pub fn resolve(&self, statistic: f64) -> f64 {
        statistic * self.factor + self.offset
    }
}

/// A 1D range gate whose bounds track a statistic of a reference population.
///
/// Unlike a [`RangeGate`], the limits are not fixed: they are recomputed from
/// the reference population whenever [`TetheredRangeGate::resolve`] is called.
/// Typical uses are "events above the parent's 95th percentile" or "events
/// within the parent median ± 2 SD". The reference may be the parent, an
/// ancestor, or any other population.
///
/// Call `resolve` after the reference population changes, before evaluating the
/// gate. An unresolved gate selects no events (its bounds are unknown), so a
/// gating hierarchy must evaluate the reference first.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TetheredRangeGate {
    pub param_idx: usize,
    pub statistic: TetherStatistic,
    pub low: TetherBound,
    pub high: TetherBound,
    /// Cached resolved bounds; not serialized because they are derived from a
    /// reference population that may change between sessions.
    #[serde(skip)]
    resolved: Option<(f32, f32)>,
}

impl PartialEq for TetheredRangeGate {
    /// Compares only the gate definition, not the cached resolution.
    fn eq(&self, other: &Self) -> bool {
        self.param_idx == other.param_idx
            && self.statistic == other.statistic
            && self.low == other.low
            && self.high == other.high
    }
}

impl TetheredRangeGate {
    /// Create a tethered gate from explicit low/high bounds.
    pub fn new(
        param_idx: usize,
        statistic: TetherStatistic,
        low: TetherBound,
        high: TetherBound,
    ) -> Self {
        Self {
            param_idx,
            statistic,
            low,
            high,
            resolved: None,
        }
    }

    /// Create a gate tethered at `statistic ± offsets`.
    pub fn with_offsets(
        param_idx: usize,
        statistic: TetherStatistic,
        low_offset: f64,
        high_offset: f64,
    ) -> Self {
        Self::new(
            param_idx,
            statistic,
            TetherBound::offset(low_offset),
            TetherBound::offset(high_offset),
        )
    }

    /// Create a gate around the median with the given absolute offsets.
    pub fn median_with_offsets(param_idx: usize, low_offset: f64, high_offset: f64) -> Self {
        Self::with_offsets(param_idx, TetherStatistic::Median, low_offset, high_offset)
    }

    /// Compute the reference statistic and cache the resulting bounds.
    ///
    /// `reference_indices` selects the reference population's events from
    /// `data`; invalid indices are ignored. Returns the resolved inclusive
    /// bounds, or `None` when the reference has no finite values (the gate is
    /// then unresolved and selects nothing).
    pub fn resolve(
        &mut self,
        data: &ndarray::Array2<f32>,
        reference_indices: &[usize],
    ) -> Option<(f32, f32)> {
        let mut values: Vec<f64> = reference_indices
            .iter()
            .filter_map(|&i| data.get((i, self.param_idx)).copied())
            .filter(|v| v.is_finite())
            .map(|v| v as f64)
            .collect();
        if values.is_empty() {
            self.resolved = None;
            return None;
        }
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let statistic = self.statistic.compute_from_sorted(&values);
        if !statistic.is_finite() {
            self.resolved = None;
            return None;
        }
        let mut low = self.low.resolve(statistic) as f32;
        let mut high = self.high.resolve(statistic) as f32;
        if low > high {
            std::mem::swap(&mut low, &mut high);
        }
        self.resolved = Some((low, high));
        Some((low, high))
    }

    /// The cached inclusive bounds, if the gate has been resolved.
    pub fn resolved_bounds(&self) -> Option<(f32, f32)> {
        self.resolved
    }

    /// True when the gate has bounds and can be evaluated.
    pub fn is_resolved(&self) -> bool {
        self.resolved.is_some()
    }

    /// Discard cached bounds, e.g. when the reference population is edited.
    pub fn clear_resolution(&mut self) {
        self.resolved = None;
    }
}

impl Gate for TetheredRangeGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        self.resolved
            .map(|(low, high)| {
                let value = event[self.param_idx];
                value >= low && value <= high
            })
            .unwrap_or(false)
    }

    fn filter(&self, data: &ndarray::Array2<f32>) -> Vec<usize> {
        let Some((low, high)) = self.resolved else {
            return Vec::new();
        };
        data.axis_iter(Axis(0))
            .enumerate()
            .filter_map(|(idx, event)| {
                let value = event[self.param_idx];
                (value >= low && value <= high).then_some(idx)
            })
            .collect()
    }

    fn parameter_indices(&self) -> Vec<usize> {
        vec![self.param_idx]
    }
}

// ---------------------------------------------------------------------------
// Spider gate (wedge/annular sector around a centre)
// ---------------------------------------------------------------------------

/// Large-but-finite default outer radius, used instead of infinity so gates
/// serialize cleanly to JSON.
const SPIDER_UNBOUNDED_RADIUS: f32 = 1.0e30;

/// A wedge or annular sector around a centre: a "spider" gate.
///
/// The gate selects events whose distance from `(center_x, center_y)` lies
/// between `min_radius` and `max_radius` and whose polar angle lies inside the
/// `start_angle..end_angle` sweep (radians, counter-clockwise from +x). Sweeps
/// may cross the ±π discontinuity and may span a full circle.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpiderGate {
    pub param_idx_x: usize,
    pub param_idx_y: usize,
    pub center_x: f32,
    pub center_y: f32,
    /// Start of the angular sweep, in radians.
    pub start_angle: f32,
    /// End of the angular sweep, in radians.
    pub end_angle: f32,
    /// Inner radius (inclusive).
    pub min_radius: f32,
    /// Outer radius (inclusive).
    pub max_radius: f32,
}

impl SpiderGate {
    /// A wedge from `start_angle` to `end_angle`, unbounded radially.
    pub fn new(
        x: usize,
        y: usize,
        center_x: f32,
        center_y: f32,
        start_angle: f32,
        end_angle: f32,
    ) -> Self {
        Self {
            param_idx_x: x,
            param_idx_y: y,
            center_x,
            center_y,
            start_angle,
            end_angle,
            min_radius: 0.0,
            max_radius: SPIDER_UNBOUNDED_RADIUS,
        }
    }

    /// A full-circle spider gate (useful as a radius-only selection).
    pub fn full_circle(x: usize, y: usize, center_x: f32, center_y: f32) -> Self {
        Self::new(x, y, center_x, center_y, 0.0, std::f32::consts::TAU)
    }

    /// Restrict the gate to an annulus between `min_radius` and `max_radius`.
    pub fn with_radii(mut self, min_radius: f32, max_radius: f32) -> Self {
        let a = min_radius.abs();
        let b = max_radius.abs();
        self.min_radius = a.min(b);
        self.max_radius = a.max(b);
        self
    }

    /// True when `angle` (radians) lies inside the configured sweep.
    fn angle_in_wedge(&self, angle: f32) -> bool {
        let raw_span = self.end_angle - self.start_angle;
        if raw_span.abs() >= std::f32::consts::TAU - 1e-6 {
            return true; // full circle
        }
        let span = raw_span.rem_euclid(std::f32::consts::TAU);
        if span == 0.0 {
            return false;
        }
        let offset = (angle - self.start_angle).rem_euclid(std::f32::consts::TAU);
        offset <= span
    }
}

impl Gate for SpiderGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        let dx = event[self.param_idx_x] - self.center_x;
        let dy = event[self.param_idx_y] - self.center_y;
        let radius = dx.hypot(dy);
        if radius < self.min_radius || radius > self.max_radius {
            return false;
        }
        self.angle_in_wedge(dy.atan2(dx))
    }

    fn parameter_indices(&self) -> Vec<usize> {
        vec![self.param_idx_x, self.param_idx_y]
    }
}

// ---------------------------------------------------------------------------
// Curly quadrant gate (quadrants with adjustable curved dividers)
// ---------------------------------------------------------------------------

/// A quadrant gate whose vertical and horizontal dividers are exponential
/// curves instead of straight lines.
///
/// Both dividers pass through `(x, y)`. The vertical divider's x position at a
/// given event is
///
/// ```text
/// x_div = x + x_curvature * sign(dy) * (exp(exponent * |dy|) - 1) / exponent
/// ```
///
/// and the horizontal divider is symmetric with `y_curvature`. At
/// `curvature == 0` the gate is identical to a [`QuadrantGate`]. Increasing a
/// curvature bows that divider outward, which lets four-way gates follow
/// non-linear population boundaries while still partitioning every event into
/// exactly one quadrant.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CurlyQuadrantGate {
    pub param_idx_x: usize,
    pub param_idx_y: usize,
    /// Divider intersection (both curves pass through this point).
    pub x: f32,
    pub y: f32,
    /// Curvature of the vertical divider (x position as a function of y).
    pub x_curvature: f32,
    /// Curvature of the horizontal divider (y position as a function of x).
    pub y_curvature: f32,
    /// Exponential steepness shared by both dividers. Values near zero behave
    /// like a straight divider; larger values make the curve grow faster.
    pub exponent: f32,
    pub quadrant: Quadrant,
}

impl CurlyQuadrantGate {
    pub fn new(x: usize, y: usize, x_split: f32, y_split: f32, quadrant: Quadrant) -> Self {
        Self {
            param_idx_x: x,
            param_idx_y: y,
            x: x_split,
            y: y_split,
            x_curvature: 0.0,
            y_curvature: 0.0,
            exponent: 1.0,
            quadrant,
        }
    }

    /// Set the two divider curvatures.
    pub fn with_curvature(mut self, x_curvature: f32, y_curvature: f32) -> Self {
        self.x_curvature = x_curvature;
        self.y_curvature = y_curvature;
        self
    }

    /// Set the exponential steepness (clamped to a positive value).
    pub fn with_exponent(mut self, exponent: f32) -> Self {
        self.exponent = exponent.max(1e-3);
        self
    }

    /// Returns all four curved quadrants as separate gates.
    pub fn all(x: usize, y: usize, x_split: f32, y_split: f32) -> [Self; 4] {
        [
            Self::new(x, y, x_split, y_split, Quadrant::UpperLeft),
            Self::new(x, y, x_split, y_split, Quadrant::UpperRight),
            Self::new(x, y, x_split, y_split, Quadrant::LowerLeft),
            Self::new(x, y, x_split, y_split, Quadrant::LowerRight),
        ]
    }

    /// Curved divider position: `split + curvature * exponential(offset)`.
    fn divider(split: f32, curvature: f32, exponent: f32, offset: f32) -> f32 {
        if curvature == 0.0 {
            return split;
        }
        let e = exponent.max(1e-3);
        let magnitude = (e * offset.abs()).exp_m1() / e;
        split + curvature * offset.signum() * magnitude
    }

    /// Which side of the vertical divider the event is on.
    fn is_right(&self, xv: f32, yv: f32) -> bool {
        let divider = Self::divider(self.x, self.x_curvature, self.exponent, yv - self.y);
        xv >= divider
    }

    /// Which side of the horizontal divider the event is on.
    fn is_up(&self, xv: f32, yv: f32) -> bool {
        let divider = Self::divider(self.y, self.y_curvature, self.exponent, xv - self.x);
        yv >= divider
    }
}

impl Gate for CurlyQuadrantGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        let xv = event[self.param_idx_x];
        let yv = event[self.param_idx_y];
        let right = self.is_right(xv, yv);
        let up = self.is_up(xv, yv);
        match self.quadrant {
            Quadrant::UpperLeft => !right && up,
            Quadrant::UpperRight => right && up,
            Quadrant::LowerLeft => !right && !up,
            Quadrant::LowerRight => right && !up,
        }
    }

    fn parameter_indices(&self) -> Vec<usize> {
        vec![self.param_idx_x, self.param_idx_y]
    }
}

// ---------------------------------------------------------------------------
// Gate geometry: conversion, scaling and styling
// ---------------------------------------------------------------------------

/// Target shape for [`GateGeometry::convert`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GateShape {
    Rectangle,
    Ellipse,
    Polygon,
}

impl GateShape {
    pub fn label(&self) -> &'static str {
        match self {
            GateShape::Rectangle => "rectangle",
            GateShape::Ellipse => "ellipse",
            GateShape::Polygon => "polygon",
        }
    }
}

/// Parameter-independent geometry of a 2D gate.
///
/// This is the interchange type behind rectangle ↔ ellipse ↔ polygon
/// conversion and uniform scaling. It deliberately carries no parameter
/// indices; attach those when converting back to a concrete gate with
/// [`GateGeometry::to_rectangular_gate`], [`GateGeometry::to_ellipse_gate`] or
/// [`GateGeometry::to_polygon_gate`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GateGeometry {
    Rectangle {
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
        /// Rotation about the rectangle's centre, in radians.
        angle: f32,
    },
    Ellipse {
        center_x: f32,
        center_y: f32,
        radius_x: f32,
        radius_y: f32,
        /// Rotation about the ellipse's centre, in radians.
        angle: f32,
    },
    Polygon {
        vertices: Vec<(f32, f32)>,
    },
}

impl GateGeometry {
    pub fn rectangle(min_x: f32, max_x: f32, min_y: f32, max_y: f32) -> Self {
        Self::Rectangle {
            min_x: min_x.min(max_x),
            max_x: max_x.max(min_x),
            min_y: min_y.min(max_y),
            max_y: max_y.max(min_y),
            angle: 0.0,
        }
    }

    pub fn ellipse(center_x: f32, center_y: f32, radius_x: f32, radius_y: f32) -> Self {
        Self::Ellipse {
            center_x,
            center_y,
            radius_x: radius_x.abs(),
            radius_y: radius_y.abs(),
            angle: 0.0,
        }
    }

    pub fn polygon(vertices: Vec<(f32, f32)>) -> Self {
        Self::Polygon { vertices }
    }

    pub fn shape(&self) -> GateShape {
        match self {
            GateGeometry::Rectangle { .. } => GateShape::Rectangle,
            GateGeometry::Ellipse { .. } => GateShape::Ellipse,
            GateGeometry::Polygon { .. } => GateShape::Polygon,
        }
    }

    /// Axis-aligned bounding box `(min_x, max_x, min_y, max_y)`.
    ///
    /// Rotation is accounted for: a rotated rectangle or ellipse returns the
    /// extent of its rotated outline, not its unrotated axes. An empty polygon
    /// returns a degenerate box at the origin.
    pub fn bounding_box(&self) -> (f32, f32, f32, f32) {
        match self {
            GateGeometry::Rectangle {
                min_x,
                max_x,
                min_y,
                max_y,
                angle,
            } => {
                if *angle == 0.0 {
                    return (*min_x, *max_x, *min_y, *max_y);
                }
                let (cx, cy) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
                let hw = (max_x - min_x) / 2.0;
                let hh = (max_y - min_y) / 2.0;
                let (s, c) = angle.sin_cos();
                let corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
                    .map(|(x, y)| (cx + x * c - y * s, cy + x * s + y * c));
                let mut min_x = f32::INFINITY;
                let mut max_x = f32::NEG_INFINITY;
                let mut min_y = f32::INFINITY;
                let mut max_y = f32::NEG_INFINITY;
                for (x, y) in corners {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
                (min_x, max_x, min_y, max_y)
            }
            GateGeometry::Ellipse {
                center_x,
                center_y,
                radius_x,
                radius_y,
                angle,
            } => {
                let (s, c) = angle.sin_cos();
                let hw = ((radius_x * c).powi(2) + (radius_y * s).powi(2)).sqrt();
                let hh = ((radius_x * s).powi(2) + (radius_y * c).powi(2)).sqrt();
                (center_x - hw, center_x + hw, center_y - hh, center_y + hh)
            }
            GateGeometry::Polygon { vertices } => {
                if vertices.is_empty() {
                    return (0.0, 0.0, 0.0, 0.0);
                }
                let mut min_x = f32::INFINITY;
                let mut max_x = f32::NEG_INFINITY;
                let mut min_y = f32::INFINITY;
                let mut max_y = f32::NEG_INFINITY;
                for &(x, y) in vertices {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
                (min_x, max_x, min_y, max_y)
            }
        }
    }

    /// Centre of the gate: rectangle/ellipse centre or polygon vertex mean.
    pub fn centroid(&self) -> (f32, f32) {
        match self {
            GateGeometry::Rectangle {
                min_x,
                max_x,
                min_y,
                max_y,
                ..
            } => ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0),
            GateGeometry::Ellipse {
                center_x, center_y, ..
            } => (*center_x, *center_y),
            GateGeometry::Polygon { vertices } => {
                if vertices.is_empty() {
                    return (0.0, 0.0);
                }
                let (sx, sy) = vertices
                    .iter()
                    .fold((0.0f32, 0.0f32), |(sx, sy), (x, y)| (sx + x, sy + y));
                (sx / vertices.len() as f32, sy / vertices.len() as f32)
            }
        }
    }

    /// Convert to an axis-aligned rectangle bounding the gate.
    pub fn to_rectangle(&self) -> GateGeometry {
        match self {
            GateGeometry::Rectangle { .. } => self.clone(),
            _ => {
                let (min_x, max_x, min_y, max_y) = self.bounding_box();
                GateGeometry::rectangle(min_x, max_x, min_y, max_y)
            }
        }
    }

    /// Convert to an ellipse inscribed in the gate's bounding box.
    pub fn to_ellipse(&self) -> GateGeometry {
        match self {
            GateGeometry::Ellipse { .. } => self.clone(),
            _ => {
                let (min_x, max_x, min_y, max_y) = self.bounding_box();
                GateGeometry::ellipse(
                    (min_x + max_x) / 2.0,
                    (min_y + max_y) / 2.0,
                    (max_x - min_x) / 2.0,
                    (max_y - min_y) / 2.0,
                )
            }
        }
    }

    /// Convert to a polygon. Ellipses are sampled with at least `segments`
    /// vertices; rectangles become their four (possibly rotated) corners.
    pub fn to_polygon(&self, segments: usize) -> GateGeometry {
        match self {
            GateGeometry::Polygon { .. } => self.clone(),
            GateGeometry::Rectangle {
                min_x,
                max_x,
                min_y,
                max_y,
                angle,
            } => {
                let (cx, cy) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
                let hw = (max_x - min_x) / 2.0;
                let hh = (max_y - min_y) / 2.0;
                let (s, c) = angle.sin_cos();
                let vertices = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
                    .map(|(x, y)| (cx + x * c - y * s, cy + x * s + y * c))
                    .to_vec();
                GateGeometry::polygon(vertices)
            }
            GateGeometry::Ellipse {
                center_x,
                center_y,
                radius_x,
                radius_y,
                angle,
            } => {
                let n = segments.max(8);
                let (s, c) = angle.sin_cos();
                let vertices = (0..n)
                    .map(|i| {
                        let theta = std::f32::consts::TAU * i as f32 / n as f32;
                        let (sin_t, cos_t) = theta.sin_cos();
                        let x = radius_x * cos_t;
                        let y = radius_y * sin_t;
                        (center_x + x * c - y * s, center_y + x * s + y * c)
                    })
                    .collect();
                GateGeometry::polygon(vertices)
            }
        }
    }

    /// Convert to `target`, preserving shape where it is already correct.
    pub fn convert(&self, target: GateShape) -> GateGeometry {
        match target {
            GateShape::Rectangle => self.to_rectangle(),
            GateShape::Ellipse => self.to_ellipse(),
            GateShape::Polygon => self.to_polygon(64),
        }
    }

    /// Scale the gate about its centroid.
    ///
    /// Factors are applied as absolute scale amounts, so a rectangle or
    /// ellipse keeps its centre and a polygon expands or contracts around its
    /// vertex mean.
    pub fn scaled(&self, factor_x: f32, factor_y: f32) -> GateGeometry {
        let fx = factor_x.abs();
        let fy = factor_y.abs();
        match self {
            GateGeometry::Rectangle {
                min_x,
                max_x,
                min_y,
                max_y,
                angle,
            } => {
                let (cx, cy) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
                let hw = (max_x - min_x) / 2.0 * fx;
                let hh = (max_y - min_y) / 2.0 * fy;
                GateGeometry::Rectangle {
                    min_x: cx - hw,
                    max_x: cx + hw,
                    min_y: cy - hh,
                    max_y: cy + hh,
                    angle: *angle,
                }
            }
            GateGeometry::Ellipse {
                center_x,
                center_y,
                radius_x,
                radius_y,
                angle,
            } => GateGeometry::Ellipse {
                center_x: *center_x,
                center_y: *center_y,
                radius_x: radius_x * fx,
                radius_y: radius_y * fy,
                angle: *angle,
            },
            GateGeometry::Polygon { vertices } => {
                if vertices.is_empty() {
                    return self.clone();
                }
                let (cx, cy) = self.centroid();
                GateGeometry::polygon(
                    vertices
                        .iter()
                        .map(|&(x, y)| (cx + (x - cx) * fx, cy + (y - cy) * fy))
                        .collect(),
                )
            }
        }
    }

    /// Build a rectangle gate on the given parameter indices.
    pub fn to_rectangular_gate(&self, x_idx: usize, y_idx: usize) -> RectangularGate {
        let GateGeometry::Rectangle {
            min_x,
            max_x,
            min_y,
            max_y,
            angle,
        } = self.to_rectangle()
        else {
            unreachable!("to_rectangle always returns a rectangle");
        };
        RectangularGate::new(x_idx, y_idx, min_x, max_x, min_y, max_y).with_angle(angle)
    }

    /// Build an ellipse gate on the given parameter indices.
    pub fn to_ellipse_gate(&self, x_idx: usize, y_idx: usize) -> EllipseGate {
        let GateGeometry::Ellipse {
            center_x,
            center_y,
            radius_x,
            radius_y,
            angle,
        } = self.to_ellipse()
        else {
            unreachable!("to_ellipse always returns an ellipse");
        };
        EllipseGate::new(x_idx, y_idx, center_x, center_y, radius_x, radius_y).with_angle(angle)
    }

    /// Build a polygon gate on the given parameter indices.
    pub fn to_polygon_gate(&self, x_idx: usize, y_idx: usize) -> PolygonGate {
        let GateGeometry::Polygon { vertices } = self.to_polygon(64) else {
            unreachable!("to_polygon always returns a polygon");
        };
        PolygonGate::new(x_idx, y_idx, vertices)
    }
}

impl RectangularGate {
    /// The gate's parameter-independent geometry.
    pub fn geometry(&self) -> GateGeometry {
        GateGeometry::Rectangle {
            min_x: self.min_x,
            max_x: self.max_x,
            min_y: self.min_y,
            max_y: self.max_y,
            angle: self.angle,
        }
    }
}

impl EllipseGate {
    /// The gate's parameter-independent geometry.
    pub fn geometry(&self) -> GateGeometry {
        GateGeometry::Ellipse {
            center_x: self.center_x,
            center_y: self.center_y,
            radius_x: self.radius_x,
            radius_y: self.radius_y,
            angle: self.angle,
        }
    }
}

impl PolygonGate {
    /// The gate's parameter-independent geometry.
    pub fn geometry(&self) -> GateGeometry {
        GateGeometry::Polygon {
            vertices: self.vertices.clone(),
        }
    }
}

/// Rendering attributes shared by gate overlays.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GateStyle {
    /// Outline thickness in points.
    pub line_width: f32,
    /// Optional RGB tint for the outline/fill.
    pub tint: Option<[u8; 3]>,
    /// Fill opacity in `[0, 1]`; zero draws outline only.
    pub fill_opacity: f32,
}

impl Default for GateStyle {
    fn default() -> Self {
        Self {
            line_width: 1.0,
            tint: None,
            fill_opacity: 0.0,
        }
    }
}

impl GateStyle {
    pub fn with_line_width(mut self, width: f32) -> Self {
        self.line_width = width.max(0.0);
        self
    }

    pub fn with_tint(mut self, rgb: [u8; 3]) -> Self {
        self.tint = Some(rgb);
        self
    }

    pub fn with_fill_opacity(mut self, opacity: f32) -> Self {
        self.fill_opacity = opacity.clamp(0.0, 1.0);
        self
    }
}

/// Median-cross overlay for a population: the median of the x parameter and
/// the median of the y parameter, in data coordinates.
///
/// Returns `None` when either parameter has no finite values for the selected
/// events. Out-of-range event indices are ignored.
pub fn median_cross(
    data: &ndarray::Array2<f32>,
    indices: &[usize],
    x_idx: usize,
    y_idx: usize,
) -> Option<(f64, f64)> {
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for &index in indices {
        if let Some(&x) = data.get((index, x_idx))
            && x.is_finite()
        {
            xs.push(x as f64);
        }
        if let Some(&y) = data.get((index, y_idx))
            && y.is_finite()
        {
            ys.push(y as f64);
        }
    }
    if xs.is_empty() || ys.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some((
        crate::statistics::percentile_sorted(&xs, 50.0),
        crate::statistics::percentile_sorted(&ys, 50.0),
    ))
}

// ---------------------------------------------------------------------------
// Boolean gate (compose gates with AND / OR / NOT)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BooleanOp {
    And,
    Or,
    Not,
}

/// Boolean composition of gates. Note: not `Clone`/`PartialEq`/`Serialize`
/// because it holds `Box<dyn Gate>`; serialize concrete gate trees manually.
#[derive(Debug)]
pub enum BooleanGate {
    /// A leaf gate.
    Leaf(Box<dyn Gate>),
    /// Combine two boolean gates.
    Binary {
        op: BooleanOp,
        left: Box<BooleanGate>,
        right: Box<BooleanGate>,
    },
    /// Negate a single gate.
    Unary {
        op: BooleanOp, // must be Not
        child: Box<BooleanGate>,
    },
}

impl BooleanGate {
    pub fn leaf<G: Gate + 'static>(gate: G) -> Self {
        Self::Leaf(Box::new(gate))
    }

    pub fn and(left: BooleanGate, right: BooleanGate) -> Self {
        Self::Binary {
            op: BooleanOp::And,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    pub fn or(left: BooleanGate, right: BooleanGate) -> Self {
        Self::Binary {
            op: BooleanOp::Or,
            left: Box::new(left),
            right: Box::new(right),
        }
    }
}

impl std::ops::Not for BooleanGate {
    type Output = BooleanGate;

    fn not(self) -> Self {
        Self::Unary {
            op: BooleanOp::Not,
            child: Box::new(self),
        }
    }
}

impl Gate for BooleanGate {
    fn contains(&self, event: ArrayView1<f32>) -> bool {
        match self {
            BooleanGate::Leaf(g) => g.contains(event),
            BooleanGate::Binary { op, left, right } => {
                let l = left.contains(event);
                let r = right.contains(event);
                match op {
                    BooleanOp::And => l && r,
                    BooleanOp::Or => l || r,
                    BooleanOp::Not => unreachable!("Not handled by Unary variant"),
                }
            }
            BooleanGate::Unary { op, child } => {
                debug_assert!(matches!(op, BooleanOp::Not));
                !child.contains(event)
            }
        }
    }
    fn parameter_indices(&self) -> Vec<usize> {
        match self {
            BooleanGate::Leaf(g) => g.parameter_indices(),
            BooleanGate::Binary { left, right, .. } => {
                let mut idx = left.parameter_indices();
                idx.extend(right.parameter_indices());
                idx
            }
            BooleanGate::Unary { child, .. } => child.parameter_indices(),
        }
    }
}

// ---------------------------------------------------------------------------
// Boolean combinations: every positive/negative pattern of several gates
// ---------------------------------------------------------------------------

/// One boolean combination of a set of gates: each input gate is either
/// required (positive) or negated (negative).
///
/// Produced by [`BooleanGate::all_combinations`].
pub struct GateCombination {
    /// For each input gate, in input order: `true` = positive, `false` = negated.
    pub pattern: Vec<bool>,
    /// The executable AND-tree implementing this pattern.
    pub gate: BooleanGate,
}

impl GateCombination {
    /// Human-readable name in FlowJo style, e.g. `CD4+ CD8-`.
    ///
    /// `labels` must contain one label per input gate; missing labels fall back
    /// to `G1`, `G2`, ...
    pub fn name(&self, labels: &[String]) -> String {
        self.pattern
            .iter()
            .enumerate()
            .map(|(i, positive)| {
                let label = labels
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("G{}", i + 1));
                format!("{label}{}", if *positive { "+" } else { "-" })
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Number of input gates required (positive) by the pattern.
    pub fn positive_count(&self) -> usize {
        self.pattern.iter().filter(|&&p| p).count()
    }
}

impl BooleanGate {
    /// Build every positive/negative combination of `gates`.
    ///
    /// For `n` gates this returns `2^n` combinations, ordered by their bit
    /// pattern (all-negative first, all-positive last). Each combination is an
    /// AND-tree of positive gates and negated (`NOT`) gates, so it can be
    /// filtered directly.
    ///
    /// The number of combinations grows exponentially; callers should restrict
    /// this to a handful of gates. Returns an empty vector when `gates` is
    /// empty or `n` is too large to represent.
    pub fn all_combinations<G>(gates: &[G]) -> Vec<GateCombination>
    where
        G: Gate + Clone + 'static,
    {
        let n = gates.len();
        if n == 0 {
            return Vec::new();
        }
        let Some(total) = 1usize.checked_shl(n as u32) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(total);
        for mask in 0..total {
            let pattern: Vec<bool> = (0..n).map(|i| mask & (1 << i) != 0).collect();
            let mut combo = Self::pattern_leaf(gates[0].clone(), pattern[0]);
            for (gate, &positive) in gates.iter().zip(pattern.iter()).skip(1) {
                combo = Self::and(combo, Self::pattern_leaf(gate.clone(), positive));
            }
            out.push(GateCombination {
                pattern,
                gate: combo,
            });
        }
        out
    }

    fn pattern_leaf<G: Gate + 'static>(gate: G, positive: bool) -> Self {
        let leaf = Self::leaf(gate);
        if positive { leaf } else { !leaf }
    }
}

// ---------------------------------------------------------------------------
// Identity / reference gates (reuse a gate elsewhere in a gate set)
// ---------------------------------------------------------------------------

/// How a [`GateSet`] entry obtains its gate.
#[derive(Debug)]
enum GateSource {
    /// A concrete gate owned by the set.
    Concrete(Box<dyn Gate>),
    /// An identity/reference gate pointing at another entry.
    Reference(usize),
}

/// A named gate entry inside a [`GateSet`].
#[derive(Debug)]
pub struct GateEntry {
    pub name: String,
    source: GateSource,
}

/// Errors while following identity/reference gates.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GateRefError {
    #[error("gate reference {0} is out of range")]
    MissingTarget(usize),
    #[error("identity-gate cycle detected: {0:?}")]
    Cycle(Vec<usize>),
}

/// A collection of named gates that supports identity/reference gates.
///
/// An identity gate reuses another entry's selection instead of duplicating its
/// geometry, similar to FlowJo reference gates. References are followed
/// recursively, and cycles are reported as [`GateRefError::Cycle`] rather than
/// recursing forever. Forward references are allowed; they are validated when
/// the gate is evaluated.
///
/// ```
/// use flowers::gating::{Gate, GateSet, RectangularGate};
/// let mut gates = GateSet::new();
/// let a = gates.add("CD3+", RectangularGate::new(0, 1, 0.0, 5.0, 0.0, 5.0));
/// let b = gates.add_reference("T cells (reused)", a);
/// assert_eq!(gates.name(b), Some("T cells (reused)"));
/// ```
#[derive(Debug, Default)]
pub struct GateSet {
    entries: Vec<GateEntry>,
}

impl GateSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a concrete gate and return its entry index.
    pub fn add<G: Gate + 'static>(&mut self, name: impl Into<String>, gate: G) -> usize {
        let index = self.entries.len();
        self.entries.push(GateEntry {
            name: name.into(),
            source: GateSource::Concrete(Box::new(gate)),
        });
        index
    }

    /// Add an identity gate referring to `target` and return its entry index.
    ///
    /// The target may be a forward reference; it is validated at evaluation
    /// time so cyclic references can be constructed and detected cleanly.
    pub fn add_reference(&mut self, name: impl Into<String>, target: usize) -> usize {
        let index = self.entries.len();
        self.entries.push(GateEntry {
            name: name.into(),
            source: GateSource::Reference(target),
        });
        index
    }

    /// Number of entries (concrete + reference gates).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entry's display name.
    pub fn name(&self, index: usize) -> Option<&str> {
        self.entries.get(index).map(|e| e.name.as_str())
    }

    /// The referenced entry index, for identity gates.
    pub fn target(&self, index: usize) -> Option<usize> {
        match self.entries.get(index)?.source {
            GateSource::Reference(target) => Some(target),
            GateSource::Concrete(_) => None,
        }
    }

    /// Follow references from `index` until a concrete gate is reached.
    fn resolve(&self, index: usize) -> Result<&dyn Gate, GateRefError> {
        let mut current = index;
        let mut seen: Vec<usize> = Vec::new();
        loop {
            if seen.contains(&current) {
                return Err(GateRefError::Cycle(seen));
            }
            seen.push(current);
            let entry = self
                .entries
                .get(current)
                .ok_or(GateRefError::MissingTarget(current))?;
            match &entry.source {
                GateSource::Concrete(gate) => return Ok(gate.as_ref()),
                GateSource::Reference(target) => current = *target,
            }
        }
    }

    /// Evaluate the entry's gate against `data`, following identity references.
    pub fn filter(
        &self,
        index: usize,
        data: &ndarray::Array2<f32>,
    ) -> Result<Vec<usize>, GateRefError> {
        Ok(self.resolve(index)?.filter(data))
    }

    /// Point-in-gate test for the entry, following identity references.
    pub fn contains(&self, index: usize, event: ArrayView1<f32>) -> Result<bool, GateRefError> {
        Ok(self.resolve(index)?.contains(event))
    }
}

// ---------------------------------------------------------------------------
// A gated population result: indices + per-parameter values for reporting.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GatedPopulation {
    pub name: String,
    pub event_indices: Vec<usize>,
    pub gate_summary: String,
}

impl GatedPopulation {
    pub fn new(name: &str, indices: Vec<usize>, gate_summary: &str) -> Self {
        Self {
            name: name.to_string(),
            event_indices: indices,
            gate_summary: gate_summary.to_string(),
        }
    }

    pub fn count(&self) -> usize {
        self.event_indices.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    fn sample_data() -> ndarray::Array2<f32> {
        // 6 events, 2 parameters
        array![
            [1.0, 1.0],
            [2.0, 2.0],
            [8.0, 1.0],
            [9.0, 2.0],
            [1.0, 8.0],
            [9.0, 9.0],
        ]
    }

    #[test]
    fn rectangular_gate_filters() {
        let data = sample_data();
        let gate = RectangularGate::new(0, 1, 0.0, 5.0, 0.0, 5.0);
        let idx = gate.filter(&data);
        assert_eq!(idx, vec![0, 1]);
    }

    #[test]
    fn rectangular_gate_rotation() {
        use ndarray::array;
        let flat = array![[0.2_f32, 0.2_f32], [5.0, 5.0]];
        let axis = RectangularGate::new(0, 1, 0.0, 10.0, 0.0, 10.0);
        assert!(axis.contains(flat.row(0)));
        let rotated = RectangularGate::new(0, 1, 0.0, 10.0, 0.0, 10.0)
            .with_angle(std::f32::consts::FRAC_PI_4);
        assert!(!rotated.contains(flat.row(0)));
        assert!(rotated.contains(flat.row(1)));
    }

    #[test]
    fn ellipse_gate_rotation() {
        use ndarray::array;
        let p = array![[5.0_f32, 8.0_f32]];
        let e0 = EllipseGate::new(0, 1, 5.0, 5.0, 4.0, 1.0);
        assert!(!e0.contains(p.row(0)));
        let e90 =
            EllipseGate::new(0, 1, 5.0, 5.0, 4.0, 1.0).with_angle(std::f32::consts::FRAC_PI_2);
        assert!(e90.contains(p.row(0)));
    }

    #[test]
    fn ellipse_gate_filters() {
        let data = sample_data();
        let gate = EllipseGate::new(0, 1, 1.5, 1.5, 1.0, 1.0);
        let idx = gate.filter(&data);
        assert_eq!(idx, vec![0, 1]);
    }

    #[test]
    fn polygon_gate_filters() {
        let data = sample_data();
        let gate = PolygonGate::new(0, 1, vec![(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0)]);
        let idx = gate.filter(&data);
        assert_eq!(idx, vec![0, 1]);
    }

    #[test]
    fn quadrant_gate_filters() {
        let data = sample_data();
        let quads = QuadrantGate::all(0, 1, 5.0, 5.0);
        let ul = quads[0].filter(&data); // x<=5, y>=5  -> event 4 [1,8]
        let ur = quads[1].filter(&data); // x>=5, y>=5  -> event 5 [9,9]
        let ll = quads[2].filter(&data); // x<=5, y<=5  -> events 0 [1,1], 1 [2,2]
        let lr = quads[3].filter(&data); // x>=5, y<=5  -> events 2 [8,1], 3 [9,2]
        assert_eq!(ul, vec![4]);
        assert_eq!(ur, vec![5]);
        assert_eq!(ll, vec![0, 1]);
        assert_eq!(lr, vec![2, 3]);
    }

    #[test]
    fn bilinear_gate_filters_quadrants() {
        let data = sample_data();
        // Center at (5,5), lines at 45deg and 135deg (an "X").
        let quads = BilinearGate::all(
            0,
            1,
            5.0,
            5.0,
            std::f32::consts::FRAC_PI_4,
            3.0 * std::f32::consts::FRAC_PI_4,
            10.0,
        );
        // (+,+): events 0 [1,1], 1 [2,2], 4 [1,8]?  Verify via the dividing lines.
        let all: Vec<Vec<usize>> = quads.iter().map(|g| g.filter(&data)).collect();
        // Every event is captured by exactly one wedge.
        let total: usize = all.iter().map(|v| v.len()).sum();
        assert_eq!(total, data.nrows());
        let mut union: Vec<usize> = all.iter().flatten().copied().collect();
        union.sort_unstable();
        assert_eq!(union, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn line_gate_selects_region_under_a_diagonal() {
        let data = sample_data();
        // Line from (0,5) to (10,5): a horizontal threshold at y=5 spanning x in [0,10].
        let below = LineGate::new(0, 1, 0.0, 5.0, 10.0, 5.0, true);
        let idx = below.filter(&data);
        // Events with y <= 5: [1,1], [2,2], [8,1], [9,2].
        assert_eq!(idx, vec![0, 1, 2, 3]);

        let above = LineGate::new(0, 1, 0.0, 5.0, 10.0, 5.0, false);
        let idx = above.filter(&data);
        assert_eq!(idx, vec![4, 5]);

        // The X extent is respected: restrict the line to x in [0,5].
        let narrow = LineGate::new(0, 1, 0.0, 5.0, 5.0, 5.0, true);
        assert_eq!(narrow.filter(&data), vec![0, 1]);
    }

    #[test]
    fn boolean_gate_combines() {
        let data = sample_data();
        let r1 = RectangularGate::new(0, 1, 0.0, 5.0, 0.0, 10.0);
        let r2 = RectangularGate::new(0, 1, 0.0, 10.0, 0.0, 5.0);
        let combo = BooleanGate::and(BooleanGate::leaf(r1), BooleanGate::leaf(r2));
        let idx = combo.filter(&data);
        assert_eq!(idx, vec![0, 1]); // x<=5 AND y<=5
    }

    #[test]
    fn range_gate_selects_one_dimensional_interval() {
        let data = sample_data();
        // Time-like parameter 0: [1, 2, 8, 9, 1, 9].
        let gate = RangeGate::new(0, 2.0, 9.0);
        assert_eq!(gate.filter(&data), vec![1, 2, 3, 5]);
        // Exclusive bounds: strictly greater than 2 and strictly less than 9.
        let gate = RangeGate::new(0, 2.0, 9.0).with_bounds(false, false);
        assert_eq!(gate.filter(&data), vec![2]);
        // Single-value interval on a constant parameter.
        let data2 = ndarray::array![[5.0], [5.0], [6.0]];
        let g2 = RangeGate::new(0, 5.0, 5.0);
        assert_eq!(g2.filter(&data2), vec![0, 1]);
    }

    #[test]
    fn time_gate_selects_value_slice() {
        let data = sample_data();
        let gate = TimeGate::new(0, 2.0, 9.0);
        assert_eq!(gate.filter(&data), vec![1, 2, 3, 5]);
        let narrow = TimeGate::new(0, 8.0, 9.0);
        assert_eq!(narrow.filter(&data), vec![2, 3, 5]);
    }

    #[test]
    fn time_gate_event_index_mode_uses_acquisition_order() {
        let data = sample_data();
        // Bounds interpreted as acquisition indices [2, 9] -> events 2,3,4,5.
        let gate = TimeGate::new(0, 2.0, 9.0).with_mode(TimeGateMode::EventIndex);
        assert_eq!(gate.filter(&data), vec![2, 3, 4, 5]);
    }

    #[test]
    fn mask_gate_sorts_deduplicates_and_filters() {
        let data = sample_data(); // 6 events
        let gate = MaskGate::new([3usize, 1, 3, 9]);
        assert_eq!(gate.indices(), &[1, 3, 9]);
        assert_eq!(gate.len(), 3);
        assert!(!gate.is_empty());
        assert!(gate.contains_index(3));
        assert!(!gate.contains_index(2));
        // Out-of-range index 9 is skipped when filtering the 6-row matrix.
        assert_eq!(gate.filter(&data), vec![1, 3]);
        assert_eq!(gate.out_of_range(6), vec![9]);
        assert!(gate.out_of_range(10).is_empty());
        // Value-based `contains` cannot decide for a mask.
        assert!(!gate.contains(data.row(1)));
    }

    #[test]
    fn mask_gate_empty_selects_nothing() {
        let data = sample_data();
        let gate = MaskGate::new(Vec::<usize>::new());
        assert!(gate.is_empty());
        assert!(gate.filter(&data).is_empty());
        assert!(gate.to_bits().is_empty());
    }

    #[test]
    fn mask_gate_bits_round_trip() {
        let gate = MaskGate::from_bits(&[0b0010_0101, 0b0000_0001]);
        // Bits 0, 2, 5 and 8 are set.
        assert_eq!(gate.indices(), &[0, 2, 5, 8]);
        assert_eq!(gate.to_bits(), vec![0b0010_0101, 0b0000_0001]);
        let back = MaskGate::from_bits(&gate.to_bits());
        assert_eq!(back, gate);
    }

    #[test]
    fn file_mask_gate_parses_indices_text() {
        let text = "# hand-picked events\n0, 2 3\n\n5;5\n6  # trailing comment\n";
        let gate = FileMaskGate::from_text("picked.txt", text).unwrap();
        assert_eq!(gate.path, std::path::PathBuf::from("picked.txt"));
        assert_eq!(gate.format, MaskFileFormat::Indices);
        assert_eq!(gate.indices(), &[0, 2, 3, 5, 6]);
        assert_eq!(gate.filter(&sample_data()), vec![0, 2, 3, 5]);
    }

    #[test]
    fn file_mask_gate_rejects_invalid_tokens() {
        let err = FileMaskGate::from_text("bad.txt", "1 2\nnope\n").unwrap_err();
        match &err {
            MaskError::InvalidIndex { token, line, .. } => {
                assert_eq!(token.as_str(), "nope");
                assert_eq!(*line, 2);
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(err.to_string().contains("line 2"));
    }

    #[test]
    fn file_mask_gate_loads_bits_and_survives_file_removal() {
        let unique = format!(
            "flowers-mask-{}-{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::write(&path, [0b0000_0101u8]).unwrap();

        let gate = FileMaskGate::load(&path, MaskFileFormat::Bits).unwrap();
        assert_eq!(gate.indices(), &[0, 2]);
        // Remove the source file: evaluation still uses the embedded mask.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(gate.filter(&sample_data()), vec![0, 2]);

        let err = FileMaskGate::load(&path, MaskFileFormat::Bits).unwrap_err();
        assert!(matches!(err, MaskError::Io { .. }));
    }

    #[test]
    fn file_mask_gate_loads_indices_from_disk() {
        let unique = format!(
            "flowers-mask-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::write(&path, "1\n4\n").unwrap();
        let gate = FileMaskGate::load(&path, MaskFileFormat::Indices).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(gate.filter(&sample_data()), vec![1, 4]);
    }

    #[test]
    fn tethered_gate_resolves_mean_offsets_against_reference() {
        let data = sample_data();
        // Reference events 0 and 1 have values [1, 2] on parameter 0.
        let mut gate = TetheredRangeGate::with_offsets(0, TetherStatistic::Mean, -0.6, 0.6);
        assert!(!gate.is_resolved());
        assert!(gate.filter(&data).is_empty());

        let bounds = gate.resolve(&data, &[0, 1]).unwrap();
        assert!((bounds.0 - 0.9).abs() < 1e-6);
        assert!((bounds.1 - 2.1).abs() < 1e-6);
        assert!(gate.is_resolved());
        assert_eq!(gate.resolved_bounds(), Some(bounds));
        // Parameter-0 values 1.0 (events 0 and 4) and 2.0 (event 1) fall in range.
        assert_eq!(gate.filter(&data), vec![0, 1, 4]);

        // Clearing the resolution makes the gate inert again.
        gate.clear_resolution();
        assert!(!gate.is_resolved());
        assert!(gate.filter(&data).is_empty());
    }

    #[test]
    fn tethered_gate_supports_factor_bounds() {
        let data = sample_data();
        // Reference events 4 and 5 have values [1, 9]; their mean is 5.
        let mut gate = TetheredRangeGate::new(
            0,
            TetherStatistic::Mean,
            TetherBound::factor(0.5), // low = 2.5
            TetherBound::factor(1.8), // high = 9.0
        );
        gate.resolve(&data, &[4, 5]).unwrap();
        assert_eq!(gate.filter(&data), vec![2, 3, 5]);
    }

    #[test]
    fn tethered_gate_supports_percentiles_and_swaps_bounds() {
        let data = sample_data();
        // 25th percentile of parameter 0 over all events is 1.25; use a wide
        // window around it and give the high bound first to test swapping.
        let mut gate =
            TetheredRangeGate::with_offsets(0, TetherStatistic::Percentile(25.0), 0.75, -0.75);
        let bounds = gate
            .resolve(&data, &(0..data.nrows()).collect::<Vec<_>>())
            .unwrap();
        assert!(bounds.0 <= bounds.1);
        assert!((bounds.0 - 0.5).abs() < 1e-6);
        assert!((bounds.1 - 2.0).abs() < 1e-6);
        assert_eq!(gate.filter(&data), vec![0, 1, 4]);
    }

    #[test]
    fn tethered_gate_handles_empty_and_invalid_references() {
        let data = sample_data();
        let mut gate = TetheredRangeGate::median_with_offsets(0, -1.0, 1.0);
        assert_eq!(gate.resolve(&data, &[]), None);
        assert!(!gate.is_resolved());
        // Out-of-range reference indices are ignored; none remain.
        assert_eq!(gate.resolve(&data, &[99, 100]), None);
        // A valid index among invalid ones still resolves.
        assert!(gate.resolve(&data, &[99, 0]).is_some());
    }

    #[test]
    fn tethered_gate_serialization_skips_resolution() {
        let data = sample_data();
        let mut gate = TetheredRangeGate::with_offsets(0, TetherStatistic::Median, -2.0, 2.0);
        gate.resolve(&data, &[0, 1, 2]);
        assert!(gate.is_resolved());

        let json = serde_json::to_string(&gate).unwrap();
        assert!(!json.contains("resolved"));
        let back: TetheredRangeGate = serde_json::from_str(&json).unwrap();
        assert_eq!(back, gate);
        assert!(!back.is_resolved());
        assert_eq!(back.param_idx, gate.param_idx);
        assert_eq!(back.statistic, gate.statistic);
        assert_eq!(back.low, gate.low);
        assert_eq!(back.high, gate.high);
    }

    #[test]
    fn tether_statistic_labels_and_bounds_helpers() {
        assert_eq!(TetherStatistic::Mean.label(), "mean");
        assert_eq!(TetherStatistic::Percentile(42.0).label(), "42nd percentile");
        assert_eq!(TetherStatistic::Percentile(11.0).label(), "11th percentile");
        assert_eq!(TetherStatistic::Percentile(95.0).label(), "95th percentile");
        assert_eq!(TetherBound::offset(3.0).resolve(10.0), 13.0);
        assert_eq!(TetherBound::factor(2.0).resolve(10.0), 20.0);
        assert_eq!(TetherBound::new(2.0, 1.0).resolve(10.0), 21.0);
    }

    #[test]
    fn gate_geometry_rect_ellipse_polygon_conversion() {
        let rect = GateGeometry::rectangle(0.0, 4.0, 2.0, 6.0);
        let ellipse = rect.to_ellipse();
        assert_eq!(ellipse, GateGeometry::ellipse(2.0, 4.0, 2.0, 2.0));
        let polygon = rect.to_polygon(64);
        let GateGeometry::Polygon { vertices } = &polygon else {
            panic!("expected polygon");
        };
        assert_eq!(vertices.len(), 4);
        assert_eq!(polygon.to_rectangle(), rect);

        // Ellipse -> polygon honors the requested segment count and bounds.
        let sampled = ellipse.to_polygon(16);
        let GateGeometry::Polygon { vertices } = &sampled else {
            panic!("expected polygon");
        };
        assert_eq!(vertices.len(), 16);
        let (min_x, max_x, min_y, max_y) = sampled.bounding_box();
        assert!((min_x - 0.0).abs() < 1e-4);
        assert!((max_x - 4.0).abs() < 1e-4);
        assert!((min_y - 2.0).abs() < 1e-4);
        assert!((max_y - 6.0).abs() < 1e-4);

        // Polygon -> ellipse uses its bounding box.
        let poly = GateGeometry::polygon(vec![(0.0, 0.0), (6.0, 0.0), (6.0, 2.0), (0.0, 2.0)]);
        assert_eq!(poly.to_ellipse(), GateGeometry::ellipse(3.0, 1.0, 3.0, 1.0));
        assert_eq!(
            poly.to_rectangle(),
            GateGeometry::rectangle(0.0, 6.0, 0.0, 2.0)
        );
        assert_eq!(rect.convert(GateShape::Ellipse), ellipse);
        assert_eq!(
            ellipse.convert(GateShape::Polygon).shape(),
            GateShape::Polygon
        );
    }

    #[test]
    fn gate_geometry_converts_back_to_concrete_gates() {
        let geometry = GateGeometry::rectangle(1.0, 5.0, 2.0, 8.0);
        let gate = geometry.to_rectangular_gate(0, 1);
        assert_eq!(
            (gate.min_x, gate.max_x, gate.min_y, gate.max_y),
            (1.0, 5.0, 2.0, 8.0)
        );
        let ellipse = geometry.to_ellipse_gate(0, 1);
        assert_eq!(
            (
                ellipse.center_x,
                ellipse.center_y,
                ellipse.radius_x,
                ellipse.radius_y
            ),
            (3.0, 5.0, 2.0, 3.0)
        );
        let polygon = geometry.to_polygon_gate(0, 1);
        assert_eq!(polygon.vertices.len(), 4);
        assert_eq!(
            polygon.geometry(),
            GateGeometry::polygon(polygon.vertices.clone())
        );

        // A concrete gate exposes its geometry.
        assert_eq!(gate.geometry(), geometry);
        let sampled = ellipse.geometry().to_polygon(12);
        assert_eq!(sampled.shape(), GateShape::Polygon);
    }

    #[test]
    fn gate_geometry_scaling_preserves_centres() {
        let rect = GateGeometry::rectangle(0.0, 4.0, 0.0, 2.0);
        assert_eq!(
            rect.scaled(2.0, 3.0),
            GateGeometry::rectangle(-2.0, 6.0, -2.0, 4.0)
        );
        let ellipse = GateGeometry::ellipse(5.0, 5.0, 2.0, 1.0);
        assert_eq!(
            ellipse.scaled(0.5, 2.0),
            GateGeometry::ellipse(5.0, 5.0, 1.0, 2.0)
        );
        let poly = GateGeometry::polygon(vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        // Centroid is (1, 1); doubling x and halving y gives (-1, 0.5) .. (3, 1.5).
        assert_eq!(
            poly.scaled(2.0, 0.5),
            GateGeometry::polygon(vec![(-1.0, 0.5), (3.0, 0.5), (3.0, 1.5), (-1.0, 1.5)])
        );
        assert_eq!(rect.centroid(), (2.0, 1.0));
        assert_eq!(poly.centroid(), (1.0, 1.0));
    }

    #[test]
    fn rotated_geometry_bounding_box_accounts_for_rotation() {
        let rect = GateGeometry::Rectangle {
            min_x: -1.0,
            max_x: 1.0,
            min_y: -1.0,
            max_y: 1.0,
            angle: std::f32::consts::FRAC_PI_4,
        };
        let (min_x, max_x, min_y, max_y) = rect.bounding_box();
        let root_two = 2.0f32.sqrt();
        assert!((min_x + root_two).abs() < 1e-5);
        assert!((max_x - root_two).abs() < 1e-5);
        assert!((min_y + root_two).abs() < 1e-5);
        assert!((max_y - root_two).abs() < 1e-5);

        // The rotated rectangle's polygon has the same extent.
        let polygon = rect.to_polygon(4);
        let (pmin_x, pmax_x, pmin_y, pmax_y) = polygon.bounding_box();
        assert!((pmin_x - min_x).abs() < 1e-5);
        assert!((pmax_x - max_x).abs() < 1e-5);
        assert!((pmin_y - min_y).abs() < 1e-5);
        assert!((pmax_y - max_y).abs() < 1e-5);
    }

    #[test]
    fn gate_style_defaults_and_builders() {
        let style = GateStyle::default();
        assert_eq!(style.line_width, 1.0);
        assert_eq!(style.tint, None);
        assert_eq!(style.fill_opacity, 0.0);

        let styled = GateStyle::default()
            .with_line_width(2.5)
            .with_tint([255, 0, 0])
            .with_fill_opacity(2.0);
        assert_eq!(styled.line_width, 2.5);
        assert_eq!(styled.tint, Some([255, 0, 0]));
        assert_eq!(styled.fill_opacity, 1.0);
        assert_eq!(
            GateStyle::default().with_fill_opacity(-1.0).fill_opacity,
            0.0
        );

        let json = serde_json::to_string(&styled).unwrap();
        assert_eq!(serde_json::from_str::<GateStyle>(&json).unwrap(), styled);
    }

    #[test]
    fn median_cross_uses_population_medians() {
        let data = sample_data();
        // All events: x median 5, y median 2.
        assert_eq!(
            median_cross(&data, &[0, 1, 2, 3, 4, 5], 0, 1),
            Some((5.0, 2.0))
        );
        // Events 0 and 1: x [1, 2] -> 1.5, y [1, 2] -> 1.5.
        assert_eq!(median_cross(&data, &[0, 1], 0, 1), Some((1.5, 1.5)));
        // Out-of-range indices and an empty population produce no overlay.
        assert_eq!(median_cross(&data, &[99], 0, 1), None);
        assert_eq!(median_cross(&data, &[], 0, 1), None);
    }

    #[test]
    fn boolean_all_combinations_cover_every_pattern() {
        let data = sample_data();
        let r1 = RectangularGate::new(0, 1, 0.0, 5.0, 0.0, 10.0); // x <= 5
        let r2 = RectangularGate::new(0, 1, 0.0, 10.0, 0.0, 5.0); // y <= 5
        let combos = BooleanGate::all_combinations(&[r1.clone(), r2.clone()]);
        assert_eq!(combos.len(), 4);
        assert_eq!(
            combos.iter().map(|c| c.pattern.clone()).collect::<Vec<_>>(),
            vec![
                vec![false, false],
                vec![true, false],
                vec![false, true],
                vec![true, true],
            ]
        );
        // All-negative: neither gate -> event 5 only.
        assert_eq!(combos[0].gate.filter(&data), vec![5]);
        // r1+ r2- -> event 4 (x <= 5, y > 5).
        assert_eq!(combos[1].gate.filter(&data), vec![4]);
        // r1- r2+ -> events 2, 3.
        assert_eq!(combos[2].gate.filter(&data), vec![2, 3]);
        // r1+ r2+ -> events 0, 1.
        assert_eq!(combos[3].gate.filter(&data), vec![0, 1]);
        assert_eq!(combos[1].positive_count(), 1);
        assert_eq!(
            combos[1].name(&["R1".to_string(), "R2".to_string()]),
            "R1+ R2-"
        );
        assert_eq!(combos[1].name(&[]), "G1+ G2-");
        // Empty input and three gates.
        assert!(BooleanGate::all_combinations::<RectangularGate>(&[]).is_empty());
        let three = BooleanGate::all_combinations(&[r1.clone(), r2, r1]);
        assert_eq!(three.len(), 8);
        assert_eq!(three[7].positive_count(), 3);
    }

    #[test]
    fn identity_gate_reuses_target_and_follows_chains() {
        let data = sample_data();
        let mut gates = GateSet::new();
        let a = gates.add("A", RectangularGate::new(0, 1, 0.0, 5.0, 0.0, 5.0));
        let b = gates.add_reference("B", a);
        let c = gates.add_reference("C", b);
        assert_eq!(gates.len(), 3);
        assert_eq!(gates.name(a), Some("A"));
        assert_eq!(gates.target(a), None);
        assert_eq!(gates.target(b), Some(a));
        assert_eq!(gates.filter(a, &data), gates.filter(b, &data));
        assert_eq!(gates.filter(b, &data), gates.filter(c, &data));
        assert_eq!(gates.filter(c, &data).unwrap(), vec![0, 1]);
        assert!(gates.contains(c, data.row(0)).unwrap());
        assert!(!gates.contains(c, data.row(2)).unwrap());
    }

    #[test]
    fn identity_gate_reports_missing_targets_and_cycles() {
        let data = sample_data();
        let mut gates = GateSet::new();
        gates.add("A", RectangularGate::new(0, 1, 0.0, 5.0, 0.0, 5.0));
        let missing = gates.add_reference("Missing", 99);
        assert_eq!(
            gates.filter(missing, &data),
            Err(GateRefError::MissingTarget(99))
        );

        // Two references pointing at each other form a cycle. Forward
        // references make this constructible without borrowing gymnastics.
        let mut cyclic = GateSet::new();
        let x = cyclic.add_reference("X", 1);
        let y = cyclic.add_reference("Y", 0);
        assert_eq!(
            cyclic.filter(x, &data),
            Err(GateRefError::Cycle(vec![x, y]))
        );
        assert!(matches!(
            cyclic.contains(y, data.row(0)),
            Err(GateRefError::Cycle(_))
        ));
        assert!(!cyclic.is_empty());
    }

    #[test]
    fn spider_gate_selects_wedge_and_annulus() {
        let data = sample_data();
        // Wedge pointing right, from -45° to +45° around (5, 5).
        let wedge = SpiderGate::new(
            0,
            1,
            5.0,
            5.0,
            -std::f32::consts::FRAC_PI_4,
            std::f32::consts::FRAC_PI_4,
        );
        // [9, 2] is at -36.9°, [9, 9] at +45° (inclusive boundary).
        assert_eq!(wedge.filter(&data), vec![3, 5]);

        // Full circle with an outer radius of 10 selects every event.
        let full = SpiderGate::full_circle(0, 1, 5.0, 5.0).with_radii(0.0, 10.0);
        assert_eq!(full.filter(&data), vec![0, 1, 2, 3, 4, 5]);

        // Annulus 2..5 around (5, 5): distances are 5.66, 4.24, 5.0, 5.0,
        // 5.0, 5.66 for events 0..5, so 4-5 is in the ring.
        let ring = SpiderGate::full_circle(0, 1, 5.0, 5.0).with_radii(5.0, 2.0);
        assert_eq!(ring.filter(&data), vec![1, 2, 3, 4]);
        assert_eq!(ring.min_radius, 2.0);
        assert_eq!(ring.max_radius, 5.0);
    }

    #[test]
    fn curly_quadrants_reduce_to_straight_quadrants() {
        let data = sample_data();
        let straight = QuadrantGate::all(0, 1, 5.0, 5.0);
        let curly = CurlyQuadrantGate::all(0, 1, 5.0, 5.0);
        for (s, c) in straight.iter().zip(curly.iter()) {
            assert_eq!(s.filter(&data), c.filter(&data));
        }
        // All four curved quadrants still partition every event exactly once.
        let all: Vec<Vec<usize>> = curly.iter().map(|g| g.filter(&data)).collect();
        let total: usize = all.iter().map(|v| v.len()).sum();
        assert_eq!(total, data.nrows());
        let mut union: Vec<usize> = all.iter().flatten().copied().collect();
        union.sort_unstable();
        assert_eq!(union, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn curly_quadrant_curvature_bends_the_divider() {
        let data = sample_data();
        let curved =
            CurlyQuadrantGate::new(0, 1, 5.0, 5.0, Quadrant::UpperRight).with_curvature(1.0, 0.0);
        // With the vertical divider bowed far to the right for high-y events,
        // [9, 9] no longer counts as "right"; the upper-left wedge gains it.
        assert!(curved.filter(&data).is_empty());
        let upper_left =
            CurlyQuadrantGate::new(0, 1, 5.0, 5.0, Quadrant::UpperLeft).with_curvature(1.0, 0.0);
        // Both high-y events lie left of the bowed divider.
        assert_eq!(upper_left.filter(&data), vec![4, 5]);

        // The exponent controls how fast the divider grows.
        let slow = CurlyQuadrantGate::new(0, 1, 5.0, 5.0, Quadrant::UpperRight)
            .with_curvature(0.01, 0.0)
            .with_exponent(0.1);
        // Small curvature keeps [9, 9] to the right of the divider.
        assert_eq!(slow.filter(&data), vec![5]);
        assert!(slow.exponent > 0.0);
    }

    #[test]
    fn spider_and_curly_gates_round_trip_through_serde() {
        let spider = SpiderGate::new(0, 1, 5.0, 5.0, 0.0, 1.0).with_radii(1.0, 9.0);
        let json = serde_json::to_string(&spider).unwrap();
        assert_eq!(serde_json::from_str::<SpiderGate>(&json).unwrap(), spider);

        let [_, curly, ..] = CurlyQuadrantGate::all(0, 1, 5.0, 5.0);
        let curly = curly.with_curvature(0.5, -0.25).with_exponent(2.0);
        let json = serde_json::to_string(&curly).unwrap();
        assert_eq!(
            serde_json::from_str::<CurlyQuadrantGate>(&json).unwrap(),
            curly
        );
    }
}
