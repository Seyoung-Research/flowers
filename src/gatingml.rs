//! Gating-ML 1.5 / 2.0 import and export.
//!
//! FlowJo can exchange gating strategies as Gating-ML XML. This module maps the
//! gate types Flowers understands to and from the standard elements:
//!
//! * `RectangleGate` — two dimensions with `min`/`max` bounds.
//! * `PolygonGate` — per-dimension vertex/coordinate lists.
//! * `EllipsoidGate` — mean vector plus a 2×2 covariance matrix (rotated
//!   ellipses included).
//! * `QuadrantGate` — divider positions plus `Quadrant` locations (`UR`, `UL`,
//!   `LR`, `LL`).
//! * `BooleanGate` — nested `and` / `or` / `not` / `gateReference` expressions.
//!
//! Parsing is deliberately tolerant: unknown elements and unsupported gate
//! types are reported through [`GatingMlDocument::warnings`] instead of being
//! silently dropped. Compensation matrices are stored in a `flowers:`
//! extension element so they survive a Flowers → Gating-ML → Flowers round
//! trip; a warning documents that other tools ignore the extension.
//!
//! Not every Flowers gate has a Gating-ML representation (line, range, time,
//! mask, spider and tethered gates do not), so conversion helpers return
//! `None` for those rather than inventing a lossy mapping.

use std::collections::HashMap;
use std::path::Path;

use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::{Reader, Writer};

use crate::compensation::CompensationMatrix;
use crate::project::GateShape;

/// Gating-ML schema version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GatingMlVersion {
    /// Gating-ML 1.5.
    V1_5,
    /// Gating-ML 2.0.
    V2_0,
}

impl GatingMlVersion {
    /// Root namespace for this version.
    pub fn namespace(&self) -> &'static str {
        match self {
            GatingMlVersion::V1_5 => "http://www.isac-net.org/std/Gating-ML/v1.5/gating",
            GatingMlVersion::V2_0 => "http://www.isac-net.org/std/Gating-ML/v2.0/gating",
        }
    }

    /// Data-type namespace used by parameters, coordinates and values.
    pub fn data_type_namespace(&self) -> &'static str {
        match self {
            GatingMlVersion::V1_5 => "http://www.isac-net.org/std/Gating-ML/v1.5/datatypes",
            GatingMlVersion::V2_0 => "http://www.isac-net.org/std/Gating-ML/v2.0/datatypes",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            GatingMlVersion::V1_5 => "1.5",
            GatingMlVersion::V2_0 => "2.0",
        }
    }
}

/// One gate in a Gating-ML document.
#[derive(Debug, Clone, PartialEq)]
pub struct GatingMlGate {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub kind: GatingMlGateKind,
}

/// Gate geometry understood by the Gating-ML mapper.
#[derive(Debug, Clone, PartialEq)]
pub enum GatingMlGateKind {
    Rectangle {
        x_param: String,
        y_param: String,
        x_min: f64,
        x_max: f64,
        y_min: f64,
        y_max: f64,
    },
    Polygon {
        x_param: String,
        y_param: String,
        vertices: Vec<[f64; 2]>,
    },
    /// An ellipse stored as a mean vector plus covariance matrix.
    Ellipse {
        x_param: String,
        y_param: String,
        center_x: f64,
        center_y: f64,
        radius_x: f64,
        radius_y: f64,
        /// Rotation of the major (x-radius) axis, in radians.
        angle: f64,
    },
    /// One wedge of a quadrant split; `quadrant` is `UR`, `UL`, `LR` or `LL`.
    Quadrant {
        x_param: String,
        y_param: String,
        x_split: f64,
        y_split: f64,
        quadrant: String,
    },
    Boolean {
        expr: BooleanExpr,
    },
}

/// Boolean gate expression tree.
#[derive(Debug, Clone, PartialEq)]
pub enum BooleanExpr {
    /// Reference to another gate by its `gating:id`.
    Ref(String),
    And(Vec<BooleanExpr>),
    Or(Vec<BooleanExpr>),
    Not(Box<BooleanExpr>),
}

/// A parsed or constructed Gating-ML document.
#[derive(Debug, Clone, PartialEq)]
pub struct GatingMlDocument {
    pub version: GatingMlVersion,
    pub gates: Vec<GatingMlGate>,
    /// Compensation matrix from the `flowers:` extension, when present.
    pub compensation: Option<CompensationMatrix>,
    /// Non-fatal import problems (unknown elements, unsupported gates, ...).
    pub warnings: Vec<String>,
}

impl GatingMlDocument {
    pub fn new(version: GatingMlVersion) -> Self {
        Self {
            version,
            gates: Vec::new(),
            compensation: None,
            warnings: Vec::new(),
        }
    }

    /// Serialize the document to Gating-ML XML.
    pub fn to_xml(&self) -> Result<String, GatingMlError> {
        let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
        let mut root = BytesStart::new("gating:Gating-ML");
        root.push_attribute(("xmlns:gating", self.version.namespace()));
        root.push_attribute(("xmlns:data-type", self.version.data_type_namespace()));
        root.push_attribute(("xmlns:flowers", "urn:flowers:gatingml"));
        root.push_attribute(("version", self.version.label()));
        write_event(&mut writer, Event::Start(root))?;

        for gate in &self.gates {
            write_gate(&mut writer, gate)?;
        }
        if let Some(matrix) = &self.compensation {
            write_compensation(&mut writer, matrix)?;
        }

        write_event(&mut writer, Event::End(BytesEnd::new("gating:Gating-ML")))?;
        String::from_utf8(writer.into_inner())
            .map_err(|e| GatingMlError::Xml(format!("generated XML is not UTF-8: {e}")))
    }

    /// Parse a Gating-ML XML document.
    ///
    /// The version is detected from the root namespace (falling back to a
    /// `version` attribute), and the root may be 1.5 or 2.0.
    pub fn from_xml(xml: &str) -> Result<Self, GatingMlError> {
        let root = parse_tree(xml)?;
        if local_name(&root.name) != "Gating-ML" {
            return Err(GatingMlError::MissingRoot);
        }
        let version = detect_version(&root);
        let mut doc = Self::new(version);

        // Compensation lives in the flowers extension (or a generic
        // compensation element emitted by other tools).
        for node in descendants(&root, "compensation") {
            match parse_compensation(node) {
                Some(matrix) => {
                    doc.compensation = Some(matrix);
                    doc.warnings.push(
                        "compensation was imported from an extension element; verify channel names"
                            .to_string(),
                    );
                }
                None => doc
                    .warnings
                    .push("a compensation element could not be parsed and was ignored".to_string()),
            }
        }

        let mut gate_nodes = Vec::new();
        collect_gate_nodes(&root, &mut gate_nodes);
        for (n, node) in gate_nodes.iter().enumerate() {
            let name = local_name(&node.name).to_string();
            match name.as_str() {
                "RectangleGate" => match parse_rectangle(node, n) {
                    Some(gate) => doc.gates.push(gate),
                    None => doc
                        .warnings
                        .push(format!("RectangleGate #{} is missing dimensions", n + 1)),
                },
                "PolygonGate" => match parse_polygon(node, n) {
                    Some(gate) => doc.gates.push(gate),
                    None => doc
                        .warnings
                        .push(format!("PolygonGate #{} has no usable vertices", n + 1)),
                },
                "EllipsoidGate" => match parse_ellipse(node, n) {
                    Some(gate) => doc.gates.push(gate),
                    None => doc.warnings.push(format!(
                        "EllipsoidGate #{} is missing mean/covariance values",
                        n + 1
                    )),
                },
                "QuadrantGate" => {
                    let gates = parse_quadrants(node, n);
                    if gates.is_empty() {
                        doc.warnings.push(format!(
                            "QuadrantGate #{} has no quadrant locations; defaulting to UR",
                            n + 1
                        ));
                        if let Some(gate) = parse_quadrant_default(node, n) {
                            doc.gates.push(gate);
                        }
                    } else {
                        doc.gates.extend(gates);
                    }
                }
                "BooleanGate" => match parse_boolean(node, n) {
                    Some(gate) => doc.gates.push(gate),
                    None => doc
                        .warnings
                        .push(format!("BooleanGate #{} has no expression", n + 1)),
                },
                _ => {}
            }
        }

        // Gate-like elements we do not implement are reported explicitly so a
        // strategy is never silently truncated.
        let mut unknown_gates = Vec::new();
        collect_unknown_gate_nodes(&root, &mut unknown_gates);
        for name in unknown_gates {
            doc.warnings
                .push(format!("unsupported gate element '{name}' was ignored"));
        }

        let gate_count = gate_nodes.len();
        let parsed = doc.gates.len();
        if gate_count > parsed
            && !doc
                .warnings
                .iter()
                .any(|w| w.contains("unsupported") || w.contains("missing"))
        {
            doc.warnings.push(format!(
                "{} gate element(s) could not be fully parsed",
                gate_count - parsed
            ));
        }
        for node in descendants(&root, "transform") {
            let id = attr(node, "id").unwrap_or("(unnamed)");
            doc.warnings.push(format!(
                "transform '{id}' was not applied: gate coordinates are stored as-is"
            ));
        }
        Ok(doc)
    }

    /// Write the document to `path`.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), GatingMlError> {
        let path = path.as_ref();
        let xml = self.to_xml()?;
        std::fs::write(path, xml).map_err(|source| GatingMlError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Read and parse a Gating-ML file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, GatingMlError> {
        let path = path.as_ref();
        let xml = std::fs::read_to_string(path).map_err(|source| GatingMlError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_xml(&xml)
    }
}

/// Errors from Gating-ML parsing, writing or file I/O.
#[derive(Debug, thiserror::Error)]
pub enum GatingMlError {
    #[error("XML error: {0}")]
    Xml(String),
    #[error("no Gating-ML root element found")]
    MissingRoot,
    #[error("failed to read or write '{}': {source}", path.display())]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn write_event(writer: &mut Writer<Vec<u8>>, event: Event<'_>) -> Result<(), GatingMlError> {
    writer
        .write_event(event)
        .map_err(|e| GatingMlError::Xml(e.to_string()))
}

// ---------------------------------------------------------------------------
// Shape conversion
// ---------------------------------------------------------------------------

/// Convert a project [`GateShape`] to Gating-ML geometry.
///
/// Returns `None` for gates Gating-ML cannot represent (line gates) and for
/// quadrant strings that are not one of `UL`/`UR`/`LL`/`LR`.
pub fn shape_to_gatingml(
    shape: &GateShape,
    x_param: impl Into<String>,
    y_param: impl Into<String>,
) -> Option<GatingMlGateKind> {
    let x_param = x_param.into();
    let y_param = y_param.into();
    match shape {
        GateShape::Rect {
            x_min,
            x_max,
            y_min,
            y_max,
            ..
        } => Some(GatingMlGateKind::Rectangle {
            x_param,
            y_param,
            x_min: *x_min,
            x_max: *x_max,
            y_min: *y_min,
            y_max: *y_max,
        }),
        GateShape::Ellipse {
            x_min,
            x_max,
            y_min,
            y_max,
            angle,
        } => Some(GatingMlGateKind::Ellipse {
            x_param,
            y_param,
            center_x: (x_min + x_max) / 2.0,
            center_y: (y_min + y_max) / 2.0,
            radius_x: (x_max - x_min).abs() / 2.0,
            radius_y: (y_max - y_min).abs() / 2.0,
            angle: *angle,
        }),
        GateShape::Polygon { vertices } => Some(GatingMlGateKind::Polygon {
            x_param,
            y_param,
            vertices: vertices.clone(),
        }),
        GateShape::Quadrant { x, y, quad } => {
            let quadrant = norm_quadrant(quad)?;
            Some(GatingMlGateKind::Quadrant {
                x_param,
                y_param,
                x_split: *x,
                y_split: *y,
                quadrant,
            })
        }
        GateShape::Bilinear { .. } => None,
        GateShape::Mask { .. } => None,
    }
}

/// Convert Gating-ML geometry back to a project [`GateShape`].
///
/// Boolean expressions have no single geometry and return `None`.
pub fn gatingml_to_shape(kind: &GatingMlGateKind) -> Option<GateShape> {
    match kind {
        GatingMlGateKind::Rectangle {
            x_min,
            x_max,
            y_min,
            y_max,
            ..
        } => Some(GateShape::Rect {
            x_min: *x_min,
            x_max: *x_max,
            y_min: *y_min,
            y_max: *y_max,
            angle: 0.0,
        }),
        GatingMlGateKind::Polygon { vertices, .. } => Some(GateShape::Polygon {
            vertices: vertices.clone(),
        }),
        GatingMlGateKind::Ellipse {
            center_x,
            center_y,
            radius_x,
            radius_y,
            angle,
            ..
        } => Some(GateShape::Ellipse {
            x_min: center_x - radius_x,
            x_max: center_x + radius_x,
            y_min: center_y - radius_y,
            y_max: center_y + radius_y,
            angle: *angle,
        }),
        GatingMlGateKind::Quadrant {
            x_split,
            y_split,
            quadrant,
            ..
        } => Some(GateShape::Quadrant {
            x: *x_split,
            y: *y_split,
            quad: norm_quadrant(quadrant)?,
        }),
        GatingMlGateKind::Boolean { .. } => None,
    }
}

fn norm_quadrant(text: &str) -> Option<String> {
    let compact: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .collect::<String>()
        .to_uppercase();
    match compact.as_str() {
        "UR" | "UPPERRIGHT" => Some("UR".to_string()),
        "UL" | "UPPERLEFT" => Some("UL".to_string()),
        "LR" | "LOWERRIGHT" => Some("LR".to_string()),
        "LL" | "LOWERLEFT" => Some("LL".to_string()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

fn write_gate(writer: &mut Writer<Vec<u8>>, gate: &GatingMlGate) -> Result<(), GatingMlError> {
    match &gate.kind {
        GatingMlGateKind::Rectangle {
            x_param,
            y_param,
            x_min,
            x_max,
            y_min,
            y_max,
        } => {
            let start = gate_start("gating:RectangleGate", gate);
            write_event(writer, Event::Start(start))?;
            write_dimension(
                writer,
                x_param,
                Some(*x_min),
                Some(*x_max),
                None::<std::iter::Empty<f64>>,
            )?;
            write_dimension(
                writer,
                y_param,
                Some(*y_min),
                Some(*y_max),
                None::<std::iter::Empty<f64>>,
            )?;
            write_event(writer, Event::End(BytesEnd::new("gating:RectangleGate")))
        }
        GatingMlGateKind::Polygon {
            x_param,
            y_param,
            vertices,
        } => {
            let start = gate_start("gating:PolygonGate", gate);
            write_event(writer, Event::Start(start))?;
            write_dimension(
                writer,
                x_param,
                None,
                None,
                Some(vertices.iter().map(|v| v[0])),
            )?;
            write_dimension(
                writer,
                y_param,
                None,
                None,
                Some(vertices.iter().map(|v| v[1])),
            )?;
            write_event(writer, Event::End(BytesEnd::new("gating:PolygonGate")))
        }
        GatingMlGateKind::Ellipse {
            x_param,
            y_param,
            center_x,
            center_y,
            radius_x,
            radius_y,
            angle,
        } => {
            let start = gate_start("gating:EllipsoidGate", gate);
            write_event(writer, Event::Start(start))?;
            for param in [x_param, y_param] {
                write_event(writer, Event::Start(BytesStart::new("gating:dimension")))?;
                write_parameter(writer, param)?;
                write_event(writer, Event::End(BytesEnd::new("gating:dimension")))?;
            }
            write_value(writer, "gating:mean", *center_x)?;
            write_value(writer, "gating:mean", *center_y)?;
            // Row-major 2x2 covariance for rx/ry at `angle`.
            let (s, c) = angle.sin_cos();
            let a = radius_x * radius_x * c * c + radius_y * radius_y * s * s;
            let d = radius_x * radius_x * s * s + radius_y * radius_y * c * c;
            let b = (radius_x * radius_x - radius_y * radius_y) * s * c;
            for value in [a, b, b, d] {
                write_value(writer, "gating:covariance", value)?;
            }
            write_event(writer, Event::End(BytesEnd::new("gating:EllipsoidGate")))
        }
        GatingMlGateKind::Quadrant {
            x_param,
            y_param,
            x_split,
            y_split,
            quadrant,
        } => {
            let start = gate_start("gating:QuadrantGate", gate);
            write_event(writer, Event::Start(start))?;
            write_dimension(writer, x_param, None, None, None::<std::iter::Empty<f64>>)?;
            // Rewrite the x dimension with its divider.
            let mut xdim = BytesStart::new("gating:dimension");
            xdim.push_attribute(("gating:divider", x_split.to_string().as_str()));
            write_event(writer, Event::Start(xdim))?;
            write_parameter(writer, x_param)?;
            write_event(writer, Event::End(BytesEnd::new("gating:dimension")))?;
            let mut ydim = BytesStart::new("gating:dimension");
            ydim.push_attribute(("gating:divider", y_split.to_string().as_str()));
            write_event(writer, Event::Start(ydim))?;
            write_parameter(writer, y_param)?;
            write_event(writer, Event::End(BytesEnd::new("gating:dimension")))?;
            let mut location = BytesStart::new("gating:Quadrant");
            location.push_attribute(("gating:location", quadrant.to_uppercase().as_str()));
            write_event(writer, Event::Empty(location))?;
            write_event(writer, Event::End(BytesEnd::new("gating:QuadrantGate")))
        }
        GatingMlGateKind::Boolean { expr } => {
            let start = gate_start("gating:BooleanGate", gate);
            write_event(writer, Event::Start(start))?;
            write_boolean_expr(writer, expr)?;
            write_event(writer, Event::End(BytesEnd::new("gating:BooleanGate")))
        }
    }
}

fn gate_start(name: &str, gate: &GatingMlGate) -> BytesStart<'static> {
    let mut start = BytesStart::new(name.to_string());
    start.push_attribute(("gating:id", gate.id.as_str()));
    if let Some(parent) = &gate.parent_id {
        start.push_attribute(("gating:parent_id", parent.as_str()));
    }
    start.push_attribute(("gating:name", gate.name.as_str()));
    start
}

fn write_dimension(
    writer: &mut Writer<Vec<u8>>,
    param: &str,
    min: Option<f64>,
    max: Option<f64>,
    vertices: Option<impl Iterator<Item = f64>>,
) -> Result<(), GatingMlError> {
    let mut dim = BytesStart::new("gating:dimension");
    if let Some(min) = min {
        dim.push_attribute(("gating:min", min.to_string().as_str()));
    }
    if let Some(max) = max {
        dim.push_attribute(("gating:max", max.to_string().as_str()));
    }
    write_event(writer, Event::Start(dim))?;
    write_parameter(writer, param)?;
    if let Some(values) = vertices {
        for value in values {
            write_event(writer, Event::Start(BytesStart::new("gating:vertex")))?;
            write_value(writer, "gating:coordinate", value)?;
            write_event(writer, Event::End(BytesEnd::new("gating:vertex")))?;
        }
    }
    write_event(writer, Event::End(BytesEnd::new("gating:dimension")))
}

fn write_parameter(writer: &mut Writer<Vec<u8>>, name: &str) -> Result<(), GatingMlError> {
    let mut param = BytesStart::new("data-type:parameter");
    param.push_attribute(("data-type:name", name));
    write_event(writer, Event::Empty(param))
}

fn write_value(
    writer: &mut Writer<Vec<u8>>,
    element: &str,
    value: f64,
) -> Result<(), GatingMlError> {
    let mut event = BytesStart::new(element.to_string());
    event.push_attribute(("data-type:value", value.to_string().as_str()));
    write_event(writer, Event::Empty(event))
}

fn write_boolean_expr(
    writer: &mut Writer<Vec<u8>>,
    expr: &BooleanExpr,
) -> Result<(), GatingMlError> {
    match expr {
        BooleanExpr::Ref(reference) => {
            let mut event = BytesStart::new("gating:gateReference");
            event.push_attribute(("gating:ref", reference.as_str()));
            write_event(writer, Event::Empty(event))
        }
        BooleanExpr::And(children) => write_boolean_list(writer, "gating:and", children),
        BooleanExpr::Or(children) => write_boolean_list(writer, "gating:or", children),
        BooleanExpr::Not(child) => {
            write_event(writer, Event::Start(BytesStart::new("gating:not")))?;
            write_boolean_expr(writer, child)?;
            write_event(writer, Event::End(BytesEnd::new("gating:not")))
        }
    }
}

fn write_boolean_list(
    writer: &mut Writer<Vec<u8>>,
    element: &str,
    children: &[BooleanExpr],
) -> Result<(), GatingMlError> {
    write_event(writer, Event::Start(BytesStart::new(element.to_string())))?;
    for child in children {
        write_boolean_expr(writer, child)?;
    }
    write_event(writer, Event::End(BytesEnd::new(element.to_string())))
}

fn write_compensation(
    writer: &mut Writer<Vec<u8>>,
    matrix: &CompensationMatrix,
) -> Result<(), GatingMlError> {
    write_event(
        writer,
        Event::Start(BytesStart::new("flowers:compensation")),
    )?;
    for channel in &matrix.channels {
        let mut event = BytesStart::new("flowers:channel");
        event.push_attribute(("name", channel.as_str()));
        write_event(writer, Event::Empty(event))?;
    }
    for row in &matrix.values {
        let values = row
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let mut event = BytesStart::new("flowers:row");
        event.push_attribute(("values", values.as_str()));
        write_event(writer, Event::Empty(event))?;
    }
    write_event(writer, Event::End(BytesEnd::new("flowers:compensation")))
}

// ---------------------------------------------------------------------------
// XML tree + parser
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct XmlNode {
    name: String,
    attrs: HashMap<String, String>,
    children: Vec<XmlNode>,
    text: String,
}

/// Parse an XML document into a simple attribute/child tree.
fn parse_tree(xml: &str) -> Result<XmlNode, GatingMlError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                let mut node = new_node(&start)?;
                node.children.clear();
                stack.push(node);
            }
            Ok(Event::Empty(start)) => {
                let node = finish_node(new_node(&start)?);
                attach(&mut stack, &mut root, node)?;
            }
            Ok(Event::End(_)) => {
                let node = stack
                    .pop()
                    .ok_or_else(|| GatingMlError::Xml("unexpected closing tag".to_string()))?;
                attach(&mut stack, &mut root, finish_node(node))?;
            }
            Ok(Event::Text(text)) => {
                if let Some(node) = stack.last_mut() {
                    let decoded = text
                        .decode()
                        .map_err(|e| GatingMlError::Xml(e.to_string()))?;
                    node.text.push_str(&decoded);
                }
            }
            Ok(Event::CData(data)) => {
                if let Some(node) = stack.last_mut() {
                    let decoded = data
                        .decode()
                        .map_err(|e| GatingMlError::Xml(e.to_string()))?;
                    node.text.push_str(&decoded);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(GatingMlError::Xml(e.to_string())),
        }
    }
    root.ok_or(GatingMlError::MissingRoot)
}

fn new_node(start: &BytesStart<'_>) -> Result<XmlNode, GatingMlError> {
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let mut attrs = HashMap::new();
    for attribute in start.attributes().with_checks(false).flatten() {
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| GatingMlError::Xml(e.to_string()))?
            .into_owned();
        attrs.insert(key, value);
    }
    Ok(XmlNode {
        name,
        attrs,
        children: Vec::new(),
        text: String::new(),
    })
}

fn finish_node(mut node: XmlNode) -> XmlNode {
    node.text = node.text.trim().to_string();
    node
}

fn attach(
    stack: &mut [XmlNode],
    root: &mut Option<XmlNode>,
    node: XmlNode,
) -> Result<(), GatingMlError> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.is_none() {
        *root = Some(node);
    } else {
        return Err(GatingMlError::Xml(
            "multiple root elements in document".to_string(),
        ));
    }
    Ok(())
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn attr<'a>(node: &'a XmlNode, local: &str) -> Option<&'a str> {
    node.attrs
        .iter()
        .find(|(key, _)| local_name(key) == local)
        .map(|(_, value)| value.as_str())
}

fn children_named<'a>(node: &'a XmlNode, local: &str) -> Vec<&'a XmlNode> {
    node.children
        .iter()
        .filter(|child| local_name(&child.name) == local)
        .collect()
}

fn descendants<'a>(node: &'a XmlNode, local: &'a str) -> Vec<&'a XmlNode> {
    let mut out = Vec::new();
    fn walk<'a>(node: &'a XmlNode, local: &str, out: &mut Vec<&'a XmlNode>) {
        for child in &node.children {
            if local_name(&child.name) == local {
                out.push(child);
            }
            walk(child, local, out);
        }
    }
    walk(node, local, &mut out);
    out
}

fn detect_version(root: &XmlNode) -> GatingMlVersion {
    for (key, value) in &root.attrs {
        let key_lower = key.to_lowercase();
        if key_lower.contains("xmlns") && value.contains("v1.5") {
            return GatingMlVersion::V1_5;
        }
    }
    if let Some(version) = attr(root, "version")
        && version.starts_with("1.5")
    {
        return GatingMlVersion::V1_5;
    }
    GatingMlVersion::V2_0
}

fn collect_unknown_gate_nodes(node: &XmlNode, out: &mut Vec<String>) {
    for child in &node.children {
        let name = local_name(&child.name);
        if name.ends_with("Gate")
            && !matches!(
                name,
                "RectangleGate" | "PolygonGate" | "EllipsoidGate" | "QuadrantGate" | "BooleanGate"
            )
        {
            out.push(name.to_string());
        }
        collect_unknown_gate_nodes(child, out);
    }
}

fn collect_gate_nodes<'a>(node: &'a XmlNode, out: &mut Vec<&'a XmlNode>) {
    for child in &node.children {
        match local_name(&child.name) {
            "RectangleGate" | "PolygonGate" | "EllipsoidGate" | "QuadrantGate" | "BooleanGate" => {
                out.push(child)
            }
            "dimension" | "vertex" | "parameter" | "mean" | "covariance" | "Quadrant" => {}
            "compensation" | "transforms" | "transform" | "channel" | "row" => {}
            _ => collect_gate_nodes(child, out),
        }
    }
}

fn gate_header(node: &XmlNode, index: usize) -> (String, Option<String>, String) {
    let id = attr(node, "id")
        .map(str::to_string)
        .unwrap_or_else(|| format!("gate-{}", index + 1));
    let parent = attr(node, "parent_id").map(str::to_string);
    let name = attr(node, "name")
        .map(str::to_string)
        .unwrap_or_else(|| id.clone());
    (id, parent, name)
}

fn dimensions(node: &XmlNode) -> Vec<&XmlNode> {
    node.children
        .iter()
        .filter(|child| local_name(&child.name) == "dimension")
        .collect()
}

fn dimension_param(dim: &XmlNode) -> Option<String> {
    descendants(dim, "parameter")
        .first()
        .and_then(|p| attr(p, "name"))
        .map(str::to_string)
}

fn coordinate_values(dim: &XmlNode) -> Vec<f64> {
    descendants(dim, "vertex")
        .into_iter()
        .filter_map(|vertex| {
            descendants(vertex, "coordinate")
                .first()
                .and_then(|c| attr(c, "value"))
                .and_then(|v| v.parse().ok())
        })
        .collect()
}

fn parse_rectangle(node: &XmlNode, index: usize) -> Option<GatingMlGate> {
    let dims = dimensions(node);
    if dims.len() < 2 {
        return None;
    }
    let (id, parent_id, name) = gate_header(node, index);
    let x_param = dimension_param(dims[0])?;
    let y_param = dimension_param(dims[1])?;
    let x_min = attr(dims[0], "min")?.parse().ok()?;
    let x_max = attr(dims[0], "max")?.parse().ok()?;
    let y_min = attr(dims[1], "min")?.parse().ok()?;
    let y_max = attr(dims[1], "max")?.parse().ok()?;
    Some(GatingMlGate {
        id,
        parent_id,
        name,
        kind: GatingMlGateKind::Rectangle {
            x_param,
            y_param,
            x_min,
            x_max,
            y_min,
            y_max,
        },
    })
}

fn parse_polygon(node: &XmlNode, index: usize) -> Option<GatingMlGate> {
    let dims = dimensions(node);
    if dims.len() < 2 {
        return None;
    }
    let (id, parent_id, name) = gate_header(node, index);
    let x_param = dimension_param(dims[0])?;
    let y_param = dimension_param(dims[1])?;
    let xs = coordinate_values(dims[0]);
    let ys = coordinate_values(dims[1]);
    let vertices = if xs.len() >= 3 && xs.len() == ys.len() {
        xs.into_iter().zip(ys).map(|(x, y)| [x, y]).collect()
    } else {
        // Some writers put paired coordinates directly under the gate.
        let pairs = children_named(node, "vertex");
        let mut vertices = Vec::new();
        for vertex in pairs {
            let coords: Vec<f64> = children_named(vertex, "coordinate")
                .into_iter()
                .filter_map(|c| attr(c, "value").and_then(|v| v.parse().ok()))
                .collect();
            if coords.len() >= 2 {
                vertices.push([coords[0], coords[1]]);
            }
        }
        vertices
    };
    if vertices.len() < 3 {
        return None;
    }
    Some(GatingMlGate {
        id,
        parent_id,
        name,
        kind: GatingMlGateKind::Polygon {
            x_param,
            y_param,
            vertices,
        },
    })
}

fn parse_ellipse(node: &XmlNode, index: usize) -> Option<GatingMlGate> {
    let dims = dimensions(node);
    if dims.len() < 2 {
        return None;
    }
    let x_param = dimension_param(dims[0])?;
    let y_param = dimension_param(dims[1])?;
    let means: Vec<f64> = descendants(node, "mean")
        .into_iter()
        .filter_map(|m| attr(m, "value").and_then(|v| v.parse().ok()))
        .collect();
    let covariances: Vec<f64> = descendants(node, "covariance")
        .into_iter()
        .filter_map(|c| attr(c, "value").and_then(|v| v.parse().ok()))
        .collect();
    if means.len() < 2 || covariances.len() < 3 {
        return None;
    }
    let (cx, cy) = (means[0], means[1]);
    let (a, b, d) = if covariances.len() >= 4 {
        (covariances[0], covariances[1], covariances[3])
    } else {
        (covariances[0], covariances[1], covariances[2])
    };
    let (radius_x, radius_y, angle) = ellipse_from_covariance(a, b, d);
    let (id, parent_id, name) = gate_header(node, index);
    Some(GatingMlGate {
        id,
        parent_id,
        name,
        kind: GatingMlGateKind::Ellipse {
            x_param,
            y_param,
            center_x: cx,
            center_y: cy,
            radius_x,
            radius_y,
            angle,
        },
    })
}

/// Eigen-decomposition of a symmetric 2×2 covariance matrix.
fn ellipse_from_covariance(a: f64, b: f64, d: f64) -> (f64, f64, f64) {
    let trace = a + d;
    let disc = (((a - d) / 2.0).powi(2) + b * b).sqrt();
    let l1 = (trace / 2.0 + disc).max(0.0);
    let l2 = (trace / 2.0 - disc).max(0.0);
    let angle = 0.5 * (2.0 * b).atan2(a - d);
    (l1.sqrt(), l2.sqrt(), angle)
}

fn parse_quadrants(node: &XmlNode, index: usize) -> Vec<GatingMlGate> {
    let dims = dimensions(node);
    if dims.len() < 2 {
        return Vec::new();
    }
    let (Some(x_param), Some(y_param)) = (dimension_param(dims[0]), dimension_param(dims[1]))
    else {
        return Vec::new();
    };
    let divider = |dim: &XmlNode| -> Option<f64> {
        descendants(dim, "divider")
            .first()
            .and_then(|d| attr(d, "value"))
            .and_then(|v| v.parse().ok())
    };
    let (Some(x_split), Some(y_split)) = (divider(dims[0]), divider(dims[1])) else {
        return Vec::new();
    };
    let (id, parent_id, name) = gate_header(node, index);
    let locations: Vec<String> = node
        .children
        .iter()
        .filter(|child| local_name(&child.name) == "Quadrant")
        .filter_map(|child| {
            attr(child, "location")
                .or_else(|| attr(child, "id"))
                .and_then(norm_quadrant)
        })
        .collect();
    locations
        .into_iter()
        .enumerate()
        .map(|(n, quadrant)| GatingMlGate {
            id: format!("{id}-{n}"),
            parent_id: parent_id.clone(),
            name: format!("{name} {quadrant}"),
            kind: GatingMlGateKind::Quadrant {
                x_param: x_param.clone(),
                y_param: y_param.clone(),
                x_split,
                y_split,
                quadrant,
            },
        })
        .collect()
}

fn parse_quadrant_default(node: &XmlNode, index: usize) -> Option<GatingMlGate> {
    let mut gates = parse_quadrants(node, index);
    gates.pop()
}

fn parse_boolean(node: &XmlNode, index: usize) -> Option<GatingMlGate> {
    let expr = node.children.iter().find_map(parse_boolean_expr)?;
    let (id, parent_id, name) = gate_header(node, index);
    Some(GatingMlGate {
        id,
        parent_id,
        name,
        kind: GatingMlGateKind::Boolean { expr },
    })
}

fn parse_boolean_expr(node: &XmlNode) -> Option<BooleanExpr> {
    match local_name(&node.name) {
        "gateReference" => attr(node, "ref").map(|r| BooleanExpr::Ref(r.to_string())),
        "and" | "or" => {
            let children: Vec<BooleanExpr> = node
                .children
                .iter()
                .filter_map(parse_boolean_expr)
                .collect();
            if children.is_empty() {
                None
            } else if local_name(&node.name) == "and" {
                Some(BooleanExpr::And(children))
            } else {
                Some(BooleanExpr::Or(children))
            }
        }
        "not" => node
            .children
            .iter()
            .find_map(parse_boolean_expr)
            .map(|child| BooleanExpr::Not(Box::new(child))),
        _ => None,
    }
}

fn parse_compensation(node: &XmlNode) -> Option<CompensationMatrix> {
    let channels: Vec<String> = descendants(node, "channel")
        .into_iter()
        .filter_map(|channel| {
            attr(channel, "name")
                .map(str::to_string)
                .or_else(|| (!channel.text.is_empty()).then(|| channel.text.clone()))
        })
        .collect();
    let rows: Vec<Vec<f32>> = descendants(node, "row")
        .into_iter()
        .filter_map(|row| {
            let text = attr(row, "values").unwrap_or(&row.text);
            let values: Vec<f32> = text
                .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
                .filter(|token| !token.is_empty())
                .filter_map(|token| token.parse().ok())
                .collect();
            (!values.is_empty()).then_some(values)
        })
        .collect();
    if rows.is_empty() {
        return None;
    }
    let channels = if channels.len() == rows.len() {
        channels
    } else {
        (1..=rows.len()).map(|i| format!("P{i}")).collect()
    };
    CompensationMatrix::new(channels, rows).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_gate() -> GatingMlGate {
        GatingMlGate {
            id: "g1".to_string(),
            parent_id: None,
            name: "Rect & <one>".to_string(),
            kind: GatingMlGateKind::Rectangle {
                x_param: "FSC-A".to_string(),
                y_param: "SSC-A".to_string(),
                x_min: 1.0,
                x_max: 9.0,
                y_min: 2.0,
                y_max: 8.0,
            },
        }
    }

    fn polygon_gate() -> GatingMlGate {
        GatingMlGate {
            id: "g2".to_string(),
            parent_id: Some("g1".to_string()),
            name: "Polygon".to_string(),
            kind: GatingMlGateKind::Polygon {
                x_param: "CD4".to_string(),
                y_param: "CD8".to_string(),
                vertices: vec![[0.0, 0.0], [4.0, 0.0], [2.0, 3.0]],
            },
        }
    }

    fn ellipse_gate() -> GatingMlGate {
        GatingMlGate {
            id: "g3".to_string(),
            parent_id: Some("g1".to_string()),
            name: "Ellipse".to_string(),
            kind: GatingMlGateKind::Ellipse {
                x_param: "X".to_string(),
                y_param: "Y".to_string(),
                center_x: 5.0,
                center_y: 6.0,
                radius_x: 2.0,
                radius_y: 1.0,
                angle: 0.3,
            },
        }
    }

    #[test]
    fn rectangle_polygon_and_ellipse_round_trip_v2() {
        let mut doc = GatingMlDocument::new(GatingMlVersion::V2_0);
        doc.gates = vec![rect_gate(), polygon_gate(), ellipse_gate()];

        let xml = doc.to_xml().unwrap();
        assert!(xml.contains("Gating-ML/v2.0/gating"));
        assert!(xml.contains("RectangleGate"));
        assert!(xml.contains("PolygonGate"));
        assert!(xml.contains("EllipsoidGate"));
        assert!(xml.contains("&amp;"));
        assert!(xml.contains("&lt;one&gt;"));

        let back = GatingMlDocument::from_xml(&xml).unwrap();
        assert_eq!(back.version, GatingMlVersion::V2_0);
        assert_eq!(back.gates.len(), 3);
        assert_eq!(back.gates[0], doc.gates[0]);
        assert_eq!(back.gates[1], doc.gates[1]);

        let GatingMlGateKind::Ellipse {
            center_x,
            center_y,
            radius_x,
            radius_y,
            angle,
            ..
        } = back.gates[2].kind
        else {
            panic!("expected ellipse");
        };
        assert!((center_x - 5.0).abs() < 1e-9);
        assert!((center_y - 6.0).abs() < 1e-9);
        assert!((radius_x - 2.0).abs() < 1e-9);
        assert!((radius_y - 1.0).abs() < 1e-9);
        assert!((angle - 0.3).abs() < 1e-9);
    }

    #[test]
    fn version_1_5_namespace_is_detected() {
        let mut doc = GatingMlDocument::new(GatingMlVersion::V1_5);
        doc.gates.push(rect_gate());
        let xml = doc.to_xml().unwrap();
        assert!(xml.contains("Gating-ML/v1.5/gating"));
        let back = GatingMlDocument::from_xml(&xml).unwrap();
        assert_eq!(back.version, GatingMlVersion::V1_5);
        assert_eq!(back.gates, doc.gates);
    }

    #[test]
    fn quadrant_gate_with_four_locations_imports_all_wedges() {
        let xml = r#"
        <gating:Gating-ML xmlns:gating="http://www.isac-net.org/std/Gating-ML/v2.0/gating"
                          xmlns:data-type="http://www.isac-net.org/std/Gating-ML/v2.0/datatypes">
          <gating:QuadrantGate gating:id="q" gating:name="Quad">
            <gating:dimension>
              <data-type:parameter data-type:name="X"/>
              <gating:divider gating:value="5"/>
            </gating:dimension>
            <gating:dimension>
              <data-type:parameter data-type:name="Y"/>
              <gating:divider gating:value="7"/>
            </gating:dimension>
            <gating:Quadrant gating:location="UR"/>
            <gating:Quadrant gating:location="UL"/>
            <gating:Quadrant gating:location="LR"/>
            <gating:Quadrant gating:location="LL"/>
          </gating:QuadrantGate>
        </gating:Gating-ML>
        "#;
        let doc = GatingMlDocument::from_xml(xml).unwrap();
        assert_eq!(doc.gates.len(), 4);
        let quads: Vec<&str> = doc
            .gates
            .iter()
            .filter_map(|g| match &g.kind {
                GatingMlGateKind::Quadrant {
                    x_split,
                    y_split,
                    quadrant,
                    ..
                } => {
                    assert_eq!((*x_split, *y_split), (5.0, 7.0));
                    Some(quadrant.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(quads, ["UR", "UL", "LR", "LL"]);
    }

    #[test]
    fn boolean_expression_round_trips() {
        let expr = BooleanExpr::And(vec![
            BooleanExpr::Ref("g1".to_string()),
            BooleanExpr::Not(Box::new(BooleanExpr::Or(vec![
                BooleanExpr::Ref("g2".to_string()),
                BooleanExpr::Ref("g3".to_string()),
            ]))),
        ]);
        let mut doc = GatingMlDocument::new(GatingMlVersion::V2_0);
        doc.gates.push(GatingMlGate {
            id: "b1".to_string(),
            parent_id: None,
            name: "Boolean".to_string(),
            kind: GatingMlGateKind::Boolean { expr },
        });
        let xml = doc.to_xml().unwrap();
        assert!(xml.contains("gateReference"));
        let back = GatingMlDocument::from_xml(&xml).unwrap();
        assert_eq!(back.gates, doc.gates);
    }

    #[test]
    fn compensation_extension_round_trips() {
        let matrix = CompensationMatrix::new(
            vec!["FITC-A".to_string(), "PE-A".to_string()],
            vec![vec![1.0, 0.1], vec![0.02, 1.0]],
        )
        .unwrap();
        let mut doc = GatingMlDocument::new(GatingMlVersion::V2_0);
        doc.gates.push(rect_gate());
        doc.compensation = Some(matrix.clone());
        let xml = doc.to_xml().unwrap();
        assert!(xml.contains("flowers:compensation"));
        assert!(xml.contains("FITC-A"));
        let back = GatingMlDocument::from_xml(&xml).unwrap();
        assert_eq!(back.compensation, Some(matrix));
        assert!(
            back.warnings.iter().any(|w| w.contains("compensation")),
            "warnings: {:?}",
            back.warnings
        );
    }

    #[test]
    fn unknown_gate_elements_produce_warnings() {
        let xml = r#"
        <gating:Gating-ML xmlns:gating="http://www.isac-net.org/std/Gating-ML/v2.0/gating">
          <gating:TriangleGate gating:id="t1"/>
        </gating:Gating-ML>
        "#;
        let doc = GatingMlDocument::from_xml(xml).unwrap();
        assert!(doc.gates.is_empty());
        assert!(
            doc.warnings.iter().any(|w| w.contains("TriangleGate")),
            "warnings: {:?}",
            doc.warnings
        );
    }

    #[test]
    fn malformed_xml_returns_an_error() {
        assert!(matches!(
            GatingMlDocument::from_xml("<gating:Gating-ML"),
            Err(GatingMlError::Xml(_))
        ));
        assert!(matches!(
            GatingMlDocument::from_xml("<not-gating/>"),
            Err(GatingMlError::MissingRoot)
        ));
    }

    #[test]
    fn project_shapes_convert_both_ways() {
        let shapes = [
            GateShape::Rect {
                x_min: 1.0,
                x_max: 5.0,
                y_min: 2.0,
                y_max: 6.0,
                angle: 0.0,
            },
            GateShape::Polygon {
                vertices: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            },
            GateShape::Ellipse {
                x_min: 0.0,
                x_max: 4.0,
                y_min: 1.0,
                y_max: 3.0,
                angle: 0.25,
            },
            GateShape::Quadrant {
                x: 5.0,
                y: 6.0,
                quad: "UR".to_string(),
            },
        ];
        for shape in shapes {
            let kind = shape_to_gatingml(&shape, "X", "Y").unwrap();
            let back = gatingml_to_shape(&kind).unwrap();
            match (&shape, &back) {
                (
                    GateShape::Ellipse {
                        x_min,
                        x_max,
                        y_min,
                        y_max,
                        angle,
                    },
                    GateShape::Ellipse {
                        x_min: bx0,
                        x_max: bx1,
                        y_min: by0,
                        y_max: by1,
                        angle: ba,
                    },
                ) => {
                    assert!((x_min - bx0).abs() < 1e-9);
                    assert!((x_max - bx1).abs() < 1e-9);
                    assert!((y_min - by0).abs() < 1e-9);
                    assert!((y_max - by1).abs() < 1e-9);
                    assert!((angle - ba).abs() < 1e-9);
                }
                _ => assert_eq!(shape, back),
            }
        }
        // Line gates are not representable.
        assert!(
            shape_to_gatingml(
                &GateShape::Bilinear {
                    x1: 0.0,
                    y1: 0.0,
                    x2: 1.0,
                    y2: 1.0,
                    below: true,
                },
                "X",
                "Y"
            )
            .is_none()
        );
        // Boolean gates have no single geometry.
        assert!(
            gatingml_to_shape(&GatingMlGateKind::Boolean {
                expr: BooleanExpr::Ref("g".to_string())
            })
            .is_none()
        );
    }

    #[test]
    fn save_and_load_round_trip() {
        let mut doc = GatingMlDocument::new(GatingMlVersion::V2_0);
        doc.gates.push(rect_gate());
        let unique = format!(
            "flowers-gatingml-{}-{}.xml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        doc.save(&path).unwrap();
        let back = GatingMlDocument::load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(back.gates, doc.gates);
        // Loading a missing file is a clean I/O error.
        assert!(matches!(
            GatingMlDocument::load(&path),
            Err(GatingMlError::Io { .. })
        ));
    }
}
