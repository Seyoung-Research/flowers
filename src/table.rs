//! Table model for the table editor and reports (Phase 6, TB1).
//!
//! A [`TableSpec`] selects the row axis (samples, populations, parameters or
//! groups) and the columns that should be rendered; [`Table`] pairs a spec with
//! the collected rows. Column providers are pluggable on top of the spec.

use serde::{Deserialize, Serialize};

use crate::derived::DerivedParameter;
use crate::statistics::{PopulationProfile, PopulationStat};

/// Row axis of a table: which entity the table's rows are drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum RowAxis {
    #[default]
    Samples,
    Populations,
    Parameters,
    Groups,
    /// One row per computed statistic (TB1 table editor).
    Statistics,
    /// One row per FCS keyword (TB1 table editor).
    Keywords,
}

/// Axis a gate coordinate/offset column refers to (TB1 table editor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GateAxis {
    #[default]
    X,
    Y,
}

/// Data source backing an editor-managed table column (TB1).
///
/// Plain columns leave [`ColumnSpec::source`] as `None` and render values from
/// the row's raw values; sourced columns are resolved automatically from a
/// [`TableRow`] context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ColumnSource {
    /// Sample identifier.
    SampleId,
    /// Population name.
    PopulationName,
    /// A statistic for a specific parameter. Use `*` as the parameter to match
    /// any parameter, or `any` as the statistic to take the first match.
    Statistic {
        parameter: String,
        statistic: String,
    },
    /// FCS keyword lookup by name (case-insensitive).
    Keyword { name: String },
    /// Low–high gate coordinates for the X or Y axis.
    GateCoordinate { axis: GateAxis },
    /// Gate width (`high - low`) for the X or Y axis.
    GateOffset { axis: GateAxis },
    /// Compensation-matrix description.
    CompensationMatrix,
    /// Transform setting for a parameter (`setting` is matched against
    /// `key=value` entries in the row's transform description).
    TransformSetting { setting: String },
    /// Graph-specification description.
    GraphSpecification,
    /// Plate well identifier (`$WELL`).
    WellId,
    /// Sample-quality report summary.
    SampleQuality,
    /// Collection date/time (`$DATE` + `$BTIM`).
    CollectionTime,
    /// Sample state (`$STATE` / `$SMNO`).
    SampleState,
    /// Reagent (`$REAGENT` / `$REAGENTS`).
    Reagent,
    /// Total event count (`$TOT`).
    TotalEvents,
    /// File URI/path (`$FIL` / `$URI`).
    FileUri,
    /// Arbitrary keyword or file metadata.
    Metadata { name: String },
}

/// A named column inside a table specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnSpec {
    pub id: String,
    pub label: String,
    /// Calculated (formula) column expression; `None` means a plain value column.
    #[serde(default)]
    pub expression: Option<String>,
    /// Value formatting applied to calculated values (TB2).
    #[serde(default)]
    pub format: ColumnFormat,
    /// Editor-managed data source; `None` renders raw row values (TB1).
    #[serde(default)]
    pub source: Option<ColumnSource>,
}

impl ColumnSpec {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            expression: None,
            format: ColumnFormat::Number,
            source: None,
        }
    }

    /// Creates a calculated column from a formula expression.
    pub fn formula(
        id: impl Into<String>,
        label: impl Into<String>,
        expression: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            expression: Some(expression.into()),
            format: ColumnFormat::Number,
            source: None,
        }
    }

    /// Creates a column backed by an editor data source (TB1).
    pub fn sourced(id: impl Into<String>, label: impl Into<String>, source: ColumnSource) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            expression: None,
            format: ColumnFormat::Number,
            source: Some(source),
        }
    }

    /// True when this column is a calculated (formula) column.
    pub fn is_formula(&self) -> bool {
        self.expression.is_some()
    }

    /// Evaluate this column for one row.
    ///
    /// Formula columns reuse the T6 derived-parameter expression engine; named
    /// column references inside the formula resolve against `row_values`, while
    /// parameter-style references (e.g. `P1`) resolve against `params`.
    pub fn compute(&self, row_values: &[(String, f64)], params: &[f64]) -> Option<f64> {
        let expression = self.expression.as_ref()?;
        let derived = DerivedParameter::new(self.id.clone(), self.label.clone(), expression);
        derived.evaluate_with_columns(params, row_values)
    }

    /// Format a calculated value according to this column's format kind.
    pub fn format_value(&self, value: f64) -> String {
        format_column_value(self.format, value)
    }
}

/// A configurable table: which row axis it uses and which columns it shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableSpec {
    pub rows: RowAxis,
    pub columns: Vec<ColumnSpec>,
}

impl TableSpec {
    pub fn new(rows: RowAxis, columns: Vec<ColumnSpec>) -> Self {
        Self { rows, columns }
    }
}

/// A table instance: its specification plus the collected rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub spec: TableSpec,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(spec: TableSpec, rows: Vec<Vec<String>>) -> Self {
        Self { spec, rows }
    }

    /// Evaluate every column of the spec for one row.
    ///
    /// Returns one `Option<f64>` per spec column: calculated columns yield
    /// their formula value, plain columns yield `None` (they are raw strings
    /// held in [`Table::rows`] and are rendered verbatim).
    pub fn computed_row(&self, row_values: &[(String, f64)], params: &[f64]) -> Vec<Option<f64>> {
        self.spec
            .columns
            .iter()
            .map(|c| c.compute(row_values, params))
            .collect()
    }

    /// Build a live table from a computed population profile: one row per
    /// statistic the profile computed, with each row laid out into the given
    /// columns (TB3 live tables with automatic recalculation).
    pub fn from_profile(profile: &PopulationProfile, columns: Vec<ColumnSpec>) -> Self {
        let mut rows: Vec<Vec<String>> = Vec::new();
        for stat in &profile.stats {
            let mut row = vec!["".to_string(); columns.len().max(1)];
            for (i, col) in columns.iter().enumerate() {
                if let Some(value) = statistic_value_for(col, stat) {
                    row[i] = col.format_value(value);
                }
            }
            rows.push(row);
        }
        let spec = TableSpec {
            rows: RowAxis::Parameters,
            columns,
        };
        Self { spec, rows }
    }

    pub fn column_count(&self) -> usize {
        self.spec.columns.len()
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Rename a column label by column id (TB2).
    pub fn rename_column(&mut self, column_id: &str, new_label: &str) {
        for column in &mut self.spec.columns {
            if column.id == column_id || column.label == column_id {
                column.label = new_label.to_string();
            }
        }
    }

    /// Column index used to resolve a named sort/filter key (TB2). Falls back
    /// to the first column when the id is not present in the spec.
    fn key_column_index(&self, column_id: &str) -> usize {
        self.spec
            .columns
            .iter()
            .position(|c| c.id == column_id)
            .unwrap_or(0)
    }

    /// Return the rows of this table sorted (stable, ascending) by the
    /// numeric value of `column_id` (TB2).
    pub fn sorted_by_column(&self, column_id: &str) -> Vec<Vec<String>> {
        let idx = self.key_column_index(column_id);
        let mut keyed: Vec<(f64, usize)> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let v = row
                    .get(idx)
                    .and_then(|c| c.parse::<f64>().ok())
                    .unwrap_or(f64::MIN);
                (v, i)
            })
            .collect();
        keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut out: Vec<Vec<String>> = Vec::with_capacity(keyed.len());
        for (_, i) in keyed {
            out.push(self.rows[i].clone());
        }
        out
    }

    /// Row indices whose value in `column_id` contains the substring `query`
    /// (find; TB2).
    pub fn find_rows(&self, column_id: &str, query: &str) -> Vec<usize> {
        let idx = self.key_column_index(column_id);
        let q = query.trim().to_ascii_lowercase();
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.get(idx)
                    .map(|c| c.to_ascii_lowercase().contains(&q))
                    .unwrap_or(false)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Keep only the rows where the value of `column_id` satisfies `keep`
    /// (filter; TB2).
    pub fn filter_rows<F: Fn(&str) -> bool>(&self, column_id: &str, keep: F) -> Vec<Vec<String>> {
        let idx = self.key_column_index(column_id);
        self.rows
            .iter()
            .filter(|row| row.get(idx).map(|c| keep(c)).unwrap_or(false))
            .cloned()
            .collect()
    }

    /// Return a copy of the rows selected by `indices` (row selection; TB2).
    pub fn selected_rows(&self, indices: &[usize]) -> Vec<Vec<String>> {
        indices
            .iter()
            .filter_map(|i| self.rows.get(*i).cloned())
            .collect()
    }

    /// Insert a row at `index`, padding it to the current column count (TB1).
    pub fn insert_row(&mut self, index: usize, mut row: Vec<String>) {
        row.resize(self.spec.columns.len(), String::new());
        let index = index.min(self.rows.len());
        self.rows.insert(index, row);
    }

    /// Delete the row at `index` (TB1). Returns false when out of bounds.
    pub fn delete_row(&mut self, index: usize) -> bool {
        if index >= self.rows.len() {
            return false;
        }
        self.rows.remove(index);
        true
    }

    /// Insert a column at `index`, adding an empty cell to every row (TB1).
    pub fn insert_column(&mut self, index: usize, column: ColumnSpec) {
        let index = index.min(self.spec.columns.len());
        self.spec.columns.insert(index, column);
        let previous_len = self.spec.columns.len().saturating_sub(1);
        for row in &mut self.rows {
            row.resize(previous_len, String::new());
            let index = index.min(row.len());
            row.insert(index, String::new());
        }
    }

    /// Delete the column at `index` from the spec and every row (TB1).
    pub fn delete_column(&mut self, index: usize) -> bool {
        if index >= self.spec.columns.len() {
            return false;
        }
        self.spec.columns.remove(index);
        for row in &mut self.rows {
            if index < row.len() {
                row.remove(index);
            }
        }
        true
    }
}

/// One row's context used to resolve editor-managed columns (TB1).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableRow {
    pub sample_id: String,
    pub population_name: String,
    pub statistics: Vec<PopulationStat>,
    /// FCS keywords (lookup is case-insensitive).
    pub keywords: std::collections::HashMap<String, String>,
    /// Gate low/high values for the X and Y axes.
    pub gate_low: Option<[f64; 2]>,
    pub gate_high: Option<[f64; 2]>,
    /// Compensation-matrix description.
    pub compensation: Option<String>,
    /// Transform description, e.g. `x=linear;y=log`.
    pub transform: Option<String>,
    /// Graph-specification description.
    pub graph_spec: Option<String>,
    /// Sample-quality report summary.
    pub quality: Option<String>,
    /// Raw values for plain columns, matched by id or label.
    pub values: Vec<(String, String)>,
}

impl TableRow {
    /// Start a row for `sample_id`.
    pub fn sample(sample_id: impl Into<String>) -> Self {
        Self {
            sample_id: sample_id.into(),
            ..Self::default()
        }
    }
}

/// Case-insensitive FCS keyword lookup (TB1).
fn row_keyword(row: &TableRow, name: &str) -> Option<String> {
    row.keywords.get(name).cloned().or_else(|| {
        row.keywords
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    })
}

/// Resolve a sourced column for one row (TB1 table editor).
pub fn column_source_value(column: &ColumnSpec, row: &TableRow) -> String {
    let source = match &column.source {
        Some(source) => source,
        None => {
            return row
                .values
                .iter()
                .find(|(key, _)| key == &column.id || key == &column.label)
                .map(|(_, value)| value.clone())
                .unwrap_or_default();
        }
    };
    match source {
        ColumnSource::SampleId => row.sample_id.clone(),
        ColumnSource::PopulationName => row.population_name.clone(),
        ColumnSource::Statistic {
            parameter,
            statistic,
        } => row
            .statistics
            .iter()
            .find(|stat| {
                (parameter == "*" || stat.parameter.eq_ignore_ascii_case(parameter))
                    && (statistic.eq_ignore_ascii_case("any")
                        || crate::statistics::statistic_label(&stat.statistic)
                            .eq_ignore_ascii_case(statistic))
            })
            .map(|stat| format_column_value(column.format, stat.value))
            .unwrap_or_default(),
        ColumnSource::Keyword { name } | ColumnSource::Metadata { name } => {
            row_keyword(row, name).unwrap_or_default()
        }
        ColumnSource::GateCoordinate { axis } => {
            let index = match axis {
                GateAxis::X => 0,
                GateAxis::Y => 1,
            };
            match (row.gate_low, row.gate_high) {
                (Some(low), Some(high)) => {
                    format!(
                        "{} - {}",
                        format_number(low[index]),
                        format_number(high[index])
                    )
                }
                _ => String::new(),
            }
        }
        ColumnSource::GateOffset { axis } => {
            let index = match axis {
                GateAxis::X => 0,
                GateAxis::Y => 1,
            };
            match (row.gate_low, row.gate_high) {
                (Some(low), Some(high)) => format_number(high[index] - low[index]),
                _ => String::new(),
            }
        }
        ColumnSource::CompensationMatrix => row.compensation.clone().unwrap_or_default(),
        ColumnSource::TransformSetting { setting } => row
            .transform
            .as_deref()
            .and_then(|transform| {
                transform.split(';').map(str::trim).find_map(|entry| {
                    let (key, value) = entry.split_once('=')?;
                    if key.trim().eq_ignore_ascii_case(setting) {
                        Some(value.trim().to_string())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_default(),
        ColumnSource::GraphSpecification => row.graph_spec.clone().unwrap_or_default(),
        ColumnSource::WellId => row_keyword(row, "$WELL").unwrap_or_default(),
        ColumnSource::SampleQuality => row.quality.clone().unwrap_or_default(),
        ColumnSource::CollectionTime => {
            let date = row_keyword(row, "$DATE").unwrap_or_default();
            let time = row_keyword(row, "$BTIM").unwrap_or_default();
            format!("{date} {time}").trim().to_string()
        }
        ColumnSource::SampleState => row_keyword(row, "$STATE")
            .or_else(|| row_keyword(row, "$SMNO"))
            .unwrap_or_default(),
        ColumnSource::Reagent => row_keyword(row, "$REAGENT")
            .or_else(|| row_keyword(row, "$REAGENTS"))
            .unwrap_or_default(),
        ColumnSource::TotalEvents => row_keyword(row, "$TOT").unwrap_or_default(),
        ColumnSource::FileUri => row_keyword(row, "$URI")
            .or_else(|| row_keyword(row, "$FIL"))
            .unwrap_or_default(),
    }
}

/// Format a gate coordinate compactly for table display (TB1).
fn format_number(value: f64) -> String {
    let rounded = format!("{value:.4}");
    if rounded.contains('.') {
        rounded
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        rounded
    }
}

/// Configurable table editor: row axis plus add/remove/reorder columns, with
/// values resolved from [`TableRow`] contexts (TB1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableEditor {
    pub rows: RowAxis,
    pub columns: Vec<ColumnSpec>,
}

impl TableEditor {
    /// Create an editor for a row axis with no columns.
    pub fn new(rows: RowAxis) -> Self {
        Self {
            rows,
            columns: Vec::new(),
        }
    }

    /// Create an editor with preconfigured columns.
    pub fn with_columns(rows: RowAxis, columns: Vec<ColumnSpec>) -> Self {
        Self { rows, columns }
    }

    /// Append a column.
    pub fn add_column(&mut self, column: ColumnSpec) -> &mut Self {
        self.columns.push(column);
        self
    }

    /// Append a column backed by an editor data source.
    pub fn add_source(
        &mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        source: ColumnSource,
    ) -> &mut Self {
        self.add_column(ColumnSpec::sourced(id, label, source))
    }

    /// Remove the column with `id` (or matching label). Returns false when not
    /// found.
    pub fn remove_column(&mut self, id: &str) -> bool {
        if let Some(index) = self
            .columns
            .iter()
            .position(|column| column.id == id || column.label == id)
        {
            self.columns.remove(index);
            true
        } else {
            false
        }
    }

    /// Move the column at `from` to `to` (both clamped to the column range).
    pub fn move_column(&mut self, from: usize, to: usize) -> bool {
        if from >= self.columns.len() {
            return false;
        }
        let column = self.columns.remove(from);
        let to = to.min(self.columns.len());
        self.columns.insert(to, column);
        true
    }

    /// Column identifiers in display order.
    pub fn column_ids(&self) -> Vec<&str> {
        self.columns
            .iter()
            .map(|column| column.id.as_str())
            .collect()
    }

    /// Build the concrete [`TableSpec`] represented by this editor.
    pub fn spec(&self) -> TableSpec {
        TableSpec::new(self.rows, self.columns.clone())
    }

    /// Render one row by resolving every configured column.
    pub fn render_row(&self, row: &TableRow) -> Vec<String> {
        self.columns
            .iter()
            .map(|column| {
                if column.is_formula() {
                    let row_values: Vec<(String, f64)> = row
                        .values
                        .iter()
                        .filter_map(|(key, value)| {
                            value
                                .parse::<f64>()
                                .ok()
                                .map(|number| (key.clone(), number))
                        })
                        .collect();
                    column
                        .compute(&row_values, &[])
                        .map(|value| column.format_value(value))
                        .unwrap_or_default()
                } else {
                    column_source_value(column, row)
                }
            })
            .collect()
    }

    /// Build a table from row contexts (TB1).
    pub fn build(&self, rows: &[TableRow]) -> Table {
        Table::new(
            self.spec(),
            rows.iter().map(|row| self.render_row(row)).collect(),
        )
    }
}

/// A live table: an editor plus the most recently recalculated result (TB3).
#[derive(Debug, Clone, PartialEq)]
pub struct LiveTable {
    pub editor: TableEditor,
    pub last: Table,
}

impl LiveTable {
    /// Create a live table with no rows.
    pub fn new(editor: TableEditor) -> Self {
        let last = editor.build(&[]);
        Self { editor, last }
    }

    /// Rebuild the table from current source rows and return the result (TB3
    /// automatic recalculation).
    pub fn recalculate(&mut self, rows: &[TableRow]) -> &Table {
        self.last = self.editor.build(rows);
        &self.last
    }

    /// The last recalculated table.
    pub fn table(&self) -> &Table {
        &self.last
    }
}

/// A set of live tables recalculated together when source data changes (TB3).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LiveTableSet {
    pub tables: Vec<LiveTable>,
}

impl LiveTableSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, table: LiveTable) -> &mut Self {
        self.tables.push(table);
        self
    }

    /// Recalculate every live table against the same source rows.
    pub fn recalculate_all(&mut self, rows: &[TableRow]) {
        for table in &mut self.tables {
            table.recalculate(rows);
        }
    }

    pub fn len(&self) -> usize {
        self.tables.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}

/// A named, reusable table template (TB3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableTemplate {
    pub name: String,
    pub spec: TableSpec,
}

impl TableTemplate {
    pub fn new(name: impl Into<String>, spec: TableSpec) -> Self {
        Self {
            name: name.into(),
            spec,
        }
    }

    /// Capture an editor configuration as a template.
    pub fn from_editor(name: impl Into<String>, editor: &TableEditor) -> Self {
        Self::new(name, editor.spec())
    }

    /// Restore an editor from this template.
    pub fn editor(&self) -> TableEditor {
        TableEditor::with_columns(self.spec.rows, self.spec.columns.clone())
    }
}

/// Library of named table templates with replace-by-name semantics (TB3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TableTemplateRegistry {
    pub templates: Vec<TableTemplate>,
}

impl TableTemplateRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Save a template, replacing any existing template with the same name.
    pub fn save(&mut self, template: TableTemplate) -> &mut Self {
        if let Some(existing) = self
            .templates
            .iter_mut()
            .find(|existing| existing.name == template.name)
        {
            *existing = template;
        } else {
            self.templates.push(template);
        }
        self
    }

    pub fn get(&self, name: &str) -> Option<&TableTemplate> {
        self.templates.iter().find(|template| template.name == name)
    }

    pub fn names(&self) -> Vec<&str> {
        self.templates
            .iter()
            .map(|template| template.name.as_str())
            .collect()
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.templates.len();
        self.templates.retain(|template| template.name != name);
        self.templates.len() != before
    }

    /// Recalculate a stored template against `rows` (TB3).
    pub fn apply(&self, name: &str, rows: &[TableRow]) -> Option<Table> {
        self.get(name).map(|template| template.editor().build(rows))
    }
}

/// A batch table output target (TB3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchTableTarget {
    /// Write to a file; the extension selects CSV, HTML, RTF, XML, SQL or XLSX.
    File(std::path::PathBuf),
    /// Copy CSV text to the system clipboard.
    Clipboard,
    /// Produce SVG suitable for insertion into a layout.
    Layout,
    /// Produce SVG for an experiment-annotation figure with the given title.
    AnnotationFigure(String),
    /// Produce a print-ready HTML document.
    Printer,
}

/// One produced batch output (TB3).
#[derive(Debug, Clone, PartialEq)]
pub struct BatchTableOutput {
    /// Human-readable target description.
    pub target: String,
    /// Produced text content (empty for binary XLSX writes).
    pub content: String,
    /// True when the output was written to an external sink (file/clipboard).
    pub written: bool,
}

fn table_default_title(table: &Table) -> &'static str {
    match table.spec.rows {
        RowAxis::Samples => "Samples",
        RowAxis::Populations => "Populations",
        RowAxis::Parameters => "Parameters",
        RowAxis::Groups => "Groups",
        RowAxis::Statistics => "Statistics",
        RowAxis::Keywords => "Keywords",
    }
}

/// Write a table to `path`, choosing the export format from the extension
/// (TB3 batch output to file).
pub fn write_table_file(table: &Table, path: &std::path::Path) -> Result<String, String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("csv")
        .to_ascii_lowercase();
    match extension.as_str() {
        "xlsx" | "xls" => {
            let bytes = table_to_xlsx(table)?;
            std::fs::write(path, bytes).map_err(|error| error.to_string())?;
            Ok(String::new())
        }
        "html" | "htm" => {
            let content = table_to_html(table);
            std::fs::write(path, &content).map_err(|error| error.to_string())?;
            Ok(content)
        }
        "rtf" => {
            let content = table_to_rtf(table);
            std::fs::write(path, &content).map_err(|error| error.to_string())?;
            Ok(content)
        }
        "xml" => {
            let content = table_to_xml(table);
            std::fs::write(path, &content).map_err(|error| error.to_string())?;
            Ok(content)
        }
        "sql" => {
            let content = table_to_sql(table);
            std::fs::write(path, &content).map_err(|error| error.to_string())?;
            Ok(content)
        }
        _ => {
            let content = table_to_csv(table);
            std::fs::write(path, &content).map_err(|error| error.to_string())?;
            Ok(content)
        }
    }
}

/// Render a table as a standalone SVG figure for layouts/annotation figures
/// (TB3).
pub fn table_to_svg(table: &Table, title: &str) -> String {
    let columns = table.spec.columns.len().max(1);
    let column_width = 120.0_f32;
    let row_height = 20.0_f32;
    let title_height = if title.is_empty() { 0.0 } else { 26.0 };
    let width = (columns as f32 * column_width).max(240.0);
    let height = title_height + row_height * (table.rows.len() as f32 + 1.0) + 8.0;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">"
    );
    if !title.is_empty() {
        svg.push_str(&format!(
            "<text x=\"4.0\" y=\"17.0\" font-family=\"sans-serif\" font-size=\"13\" font-weight=\"bold\" fill=\"#222222\">{}</text>",
            escape_xml(title)
        ));
    }
    svg.push_str(&format!(
        "<rect x=\"0\" y=\"{title_height}\" width=\"{width}\" height=\"{}\" fill=\"#f2f2f2\" stroke=\"#999999\" stroke-width=\"1\"/>",
        row_height
    ));
    for (index, column) in table.spec.columns.iter().enumerate() {
        let x = index as f32 * column_width + 6.0;
        svg.push_str(&format!(
            "<text x=\"{x}\" y=\"{}\" font-family=\"sans-serif\" font-size=\"11\" fill=\"#222222\">{}</text>",
            title_height + 14.0,
            escape_xml(&column.label)
        ));
    }
    for (row_index, row) in table.rows.iter().enumerate() {
        let y = title_height + row_height * (row_index as f32 + 1.0);
        svg.push_str(&format!(
            "<rect x=\"0\" y=\"{y}\" width=\"{width}\" height=\"{row_height}\" fill=\"none\" stroke=\"#dddddd\" stroke-width=\"1\"/>"
        ));
        for (column_index, value) in row.iter().enumerate() {
            if column_index >= columns {
                break;
            }
            let x = column_index as f32 * column_width + 6.0;
            svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{}\" font-family=\"sans-serif\" font-size=\"10\" fill=\"#333333\">{}</text>",
                y + 14.0,
                escape_xml(value)
            ));
        }
    }
    svg.push_str("</svg>");
    svg
}

/// Build a table layout object for the layout editor (TB3 batch output to
/// layout).
pub fn table_layout_object(
    table: &Table,
    title: &str,
    x: f32,
    y: f32,
    width: f32,
) -> crate::layout::LayoutObject {
    let height = 24.0 + (table.rows.len() as f32 + 1.0) * 18.0;
    crate::layout::LayoutObject::Table(crate::layout::LayoutTable::new(
        x,
        y,
        width,
        height,
        title,
        table
            .spec
            .columns
            .iter()
            .map(|column| column.label.clone())
            .collect(),
        table.rows.clone(),
    ))
}

fn table_print_html(table: &Table, title: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>@media print{{body{{margin:0}}}}table{{border-collapse:collapse}}td,th{{border:1px solid #999;padding:4px}}</style></head><body>{}</body></html>",
        escape_html(title),
        table_to_html(table)
    )
}

/// Export one table to a batch of targets (TB3). File and clipboard targets
/// are written immediately; layout, annotation-figure and printer targets
/// return the generated document so it can be placed or sent to a printer by
/// the caller.
pub fn export_table_batch(
    table: &Table,
    targets: &[BatchTableTarget],
) -> Result<Vec<BatchTableOutput>, String> {
    let mut outputs = Vec::new();
    for target in targets {
        match target {
            BatchTableTarget::File(path) => {
                let content = write_table_file(table, path)?;
                outputs.push(BatchTableOutput {
                    target: format!("file:{}", path.display()),
                    content,
                    written: true,
                });
            }
            BatchTableTarget::Clipboard => {
                copy_table_to_clipboard(table)?;
                outputs.push(BatchTableOutput {
                    target: "clipboard".into(),
                    content: table_to_csv(table),
                    written: true,
                });
            }
            BatchTableTarget::Layout => {
                outputs.push(BatchTableOutput {
                    target: "layout".into(),
                    content: table_to_svg(table, table_default_title(table)),
                    written: false,
                });
            }
            BatchTableTarget::AnnotationFigure(title) => {
                outputs.push(BatchTableOutput {
                    target: format!("annotation:{title}"),
                    content: table_to_svg(table, title),
                    written: false,
                });
            }
            BatchTableTarget::Printer => {
                outputs.push(BatchTableOutput {
                    target: "printer".into(),
                    content: table_print_html(table, table_default_title(table)),
                    written: false,
                });
            }
        }
    }
    Ok(outputs)
}

/// Formatting kinds for calculated column values (TB2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ColumnFormat {
    #[default]
    Number,
    Percent,
    Scientific,
    Text,
}

impl ColumnFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Percent => "percent",
            Self::Scientific => "scientific",
            Self::Text => "text",
        }
    }
}

impl std::str::FromStr for ColumnFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_ascii_lowercase();
        let kind = match normalized.as_str() {
            "number" | "numeric" => Self::Number,
            "percent" | "percentage" => Self::Percent,
            "scientific" | "sci" => Self::Scientific,
            "text" => Self::Text,
            _ => return Err(format!("unknown column format: {s}")),
        };
        Ok(kind)
    }
}

/// Resolve the statistic value that a table column displays for one profile
/// statistic (TB3 live tables): a column matches when its id or label equals the
/// parameter name or the statistic label.
fn statistic_value_for(col: &ColumnSpec, stat: &PopulationStat) -> Option<f64> {
    let parameter = stat.parameter.to_lowercase();
    if col.id.to_lowercase() == parameter || col.label.to_lowercase() == parameter {
        return Some(stat.value);
    }
    let stat_key = crate::statistics::statistic_label(&stat.statistic).to_lowercase();
    if col.id.to_lowercase() == stat_key || col.label.to_lowercase() == stat_key {
        return Some(stat.value);
    }
    None
}

/// Format a calculated value according to the column's format kind.
pub fn format_column_value(format: ColumnFormat, value: f64) -> String {
    match format {
        ColumnFormat::Number => {
            let rounded = format!("{:.4}", value);
            if rounded.contains('.') {
                rounded
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            } else {
                rounded
            }
        }
        ColumnFormat::Percent => format!("{:.2}%", value * 100.0),
        ColumnFormat::Scientific => format!("{:.3e}", value),
        ColumnFormat::Text => value.to_string(),
    }
}

/// Export a table to CSV text: a header row from the first column label followed
/// by one row per data row (TB6 table export).
pub fn table_to_csv(table: &Table) -> String {
    let mut out = String::new();
    if let Some(first) = table.spec.columns.first() {
        out.push_str(&first.label);
        out.push('\n');
    }
    for row in &table.rows {
        let joined = row
            .iter()
            .map(|c| c.replace(',', ";"))
            .collect::<Vec<String>>()
            .join(",");
        out.push_str(&joined);
        out.push('\n');
    }
    out
}

/// Escape a string for inclusion in HTML output (TB6 table export).
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Export a table to an HTML document (TB6 table export): a simple `<table>`
/// with a header row from the first column label and one row per data row.
pub fn table_to_html(table: &Table) -> String {
    let header = table
        .spec
        .columns
        .first()
        .map(|c| c.label.clone())
        .unwrap_or_default();
    let mut body = String::new();
    for row in &table.rows {
        let cells = row
            .iter()
            .map(|c| format!("<td>{}</td>", escape_html(c)))
            .collect::<Vec<String>>()
            .join("");
        body.push_str(&format!("<tr>{}</tr>", cells));
    }
    format!(
        "<table><thead><tr><th>{}</th></tr></thead><tbody>{}</tbody></table>",
        escape_html(&header),
        body
    )
}

/// Export a table to an RTF document (TB6 table export): a one-column-per-row
/// table using simple tab-separated runs.
pub fn table_to_rtf(table: &Table) -> String {
    let header = table
        .spec
        .columns
        .first()
        .map(|c| c.label.clone())
        .unwrap_or_default();
    let mut out = String::from(r"{\rtf1\ansi\ansicpg1252\deff0{\fonttbl{\f0 Courier;}}");
    out.push_str(r"\pard\sa0\f0 10pt ");
    out.push_str(&format!(r"{}\tab\par", header));
    for row in &table.rows {
        let cells = row
            .iter()
            .map(|c| format!(r"{}\tab", c))
            .collect::<Vec<String>>()
            .join("");
        out.push_str(&cells);
        out.push_str(r"\par");
    }
    out.push('}');
    out
}

pub fn table_to_xml(table: &Table) -> String {
    let header = table
        .spec
        .columns
        .first()
        .map(|c| c.label.clone())
        .unwrap_or_default();
    let mut body = String::new();
    for row in &table.rows {
        let cells = row
            .iter()
            .map(|c| format!("<cell>{}</cell>", escape_xml(c)))
            .collect::<Vec<String>>()
            .join("");
        body.push_str(&format!("<row>{}</row>", cells));
    }
    format!(
        "<table><header>{}</header><rows>{}</rows></table>",
        escape_xml(&header),
        body
    )
}

/// Escape a string for inclusion in XML output (TB6 table export).
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Escape a string for inclusion in a SQL single-quoted literal (TB6 table export: SQL).
fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}

/// Sanitize a column label into a SQL identifier (alphanumerics/underscore, `c<N>` fallback).
fn sql_ident(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = if cleaned.is_empty() {
        "col".to_string()
    } else {
        cleaned
    };

    if cleaned.chars().next().unwrap().is_ascii_digit() {
        format!("c{cleaned}")
    } else {
        cleaned
    }
}

/// Export a table to SQL: a `CREATE TABLE` statement followed by one
/// `INSERT` per data row (TB6 table export: SQL).
pub fn table_to_sql(table: &Table) -> String {
    if table.spec.columns.is_empty() {
        return String::new();
    }
    let table_name = "flowers_table";
    let columns: Vec<String> = table
        .spec
        .columns
        .iter()
        .map(|c| sql_ident(&c.label))
        .collect();
    let mut out = format!(
        "CREATE TABLE {} ({});
",
        table_name,
        columns.join(", ")
    );
    for row in &table.rows {
        let cols = columns.join(", ");
        let vals = row
            .iter()
            .map(|c| format!("'{}'", escape_sql(c)))
            .collect::<Vec<String>>()
            .join(", ");
        out.push_str(&format!(
            "INSERT INTO {} ({}) VALUES ({});
",
            table_name, cols, vals
        ));
    }
    out
}

/// Copy a table to the system clipboard as CSV text (TB6 table export: clipboard).
pub fn copy_table_to_clipboard(table: &Table) -> Result<(), String> {
    let text = table_to_csv(table);
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("clipboard unavailable: {e}"))?;
    clipboard
        .set_text(&text)
        .map_err(|e| format!("clipboard write failed: {e}"))?;
    Ok(())
}

/// Export a table to an Excel (XLSX) workbook as bytes (TB6 table export: Excel/XLS).
pub fn table_to_xlsx(table: &Table) -> Result<Vec<u8>, String> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("Table").map_err(|e| e.to_string())?;
    let mut row_num = 0usize;
    if let Some(first) = table.spec.columns.first() {
        worksheet
            .write_row(0, 0, std::iter::once(first.label.clone()))
            .map_err(|e| e.to_string())?;
        row_num += 1;
    }
    for row in &table.rows {
        let cells: Vec<String> = row.to_vec();
        worksheet
            .write_row(row_num as u32, 0, cells)
            .map_err(|e| e.to_string())?;
        row_num += 1;
    }
    workbook.save_to_buffer().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::statistics::Statistic;

    #[test]
    fn table_spec_round_trips_and_selects_row_axis() {
        let spec = TableSpec::new(
            RowAxis::Parameters,
            vec![
                ColumnSpec {
                    id: "P1".into(),
                    label: "FITC".into(),
                    expression: None,
                    format: ColumnFormat::Number,
                    source: None,
                },
                ColumnSpec {
                    id: "P2".into(),
                    label: "PE".into(),
                    expression: None,
                    format: ColumnFormat::Number,
                    source: None,
                },
            ],
        );
        assert_eq!(spec.rows, RowAxis::Parameters);
        assert_eq!(spec.columns.len(), 2);

        let json = serde_json::to_string(&spec).unwrap();
        let back: TableSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.rows, RowAxis::Parameters);
        assert_eq!(back.columns.len(), 2);

        let groups = TableSpec::new(RowAxis::Groups, vec![]);
        assert_eq!(groups.rows, RowAxis::Groups);
        assert_eq!(groups.columns.len(), 0);
    }

    #[test]
    fn formula_columns_evaluate_from_column_references() {
        let spec = TableSpec::new(
            RowAxis::Samples,
            vec![ColumnSpec::formula("CFU", "CFU", "FITC / PE")],
        );
        let table = Table::new(spec, vec![vec!["CFU".to_string(), "5".to_string()]]);
        let row_values = vec![("FITC".to_string(), 10.0), ("PE".to_string(), 2.0)];
        assert_eq!(
            table.computed_row(&row_values, &[0.0, 0.0]),
            vec![Some(5.0)]
        );
    }

    #[test]
    fn formula_columns_evaluate_from_parameter_references() {
        let spec = TableSpec::new(
            RowAxis::Parameters,
            vec![ColumnSpec::formula("Ratio", "Ratio", "2 * P1 + P2")],
        );
        let table = Table::new(spec, vec![vec!["Ratio".to_string(), "10".to_string()]]);
        assert_eq!(table.computed_row(&[], &[3.0, 4.0]), vec![Some(10.0)]);
    }

    #[test]
    fn plain_columns_are_not_calculated() {
        let spec = TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("S1", "sample-1")]);
        let table = Table::new(spec, vec![vec!["S1".to_string(), "100".to_string()]]);
        assert!(!table.spec.columns[0].is_formula());
        assert_eq!(table.computed_row(&[], &[]), vec![None]);
        assert_eq!(
            table.computed_row(&[("S1".to_string(), 100.0)], &[]),
            vec![None]
        );
    }

    #[test]
    fn table_holds_spec_and_rows() {
        let spec = TableSpec::new(
            RowAxis::Samples,
            vec![ColumnSpec {
                id: "S1".into(),
                label: "sample-1".into(),
                expression: None,
                format: ColumnFormat::Number,
                source: None,
            }],
        );
        let table = Table::new(spec, vec![vec!["S1".to_string(), "100".to_string()]]);
        assert_eq!(table.row_count(), 1);
        assert_eq!(table.column_count(), 1);
        assert_eq!(table.rows[0][1], "100");
    }
    #[test]
    fn rename_column_updates_the_label() {
        let spec = TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("S1", "sample-1")]);
        let mut table = Table::new(spec, vec![vec!["S1".to_string(), "100".to_string()]]);
        table.rename_column("S1", "sample-renamed");
        assert_eq!(table.spec.columns[0].label, "sample-renamed");
        assert_eq!(table.spec.columns[0].id, "S1");
    }

    #[test]
    fn sorted_by_column_orders_numeric_values() {
        let spec = TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("V", "value")]);
        let table = Table::new(
            spec,
            vec![
                vec!["3".to_string()],
                vec!["1".to_string()],
                vec!["2".to_string()],
            ],
        );
        let sorted = table.sorted_by_column("V");
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0][0], "1");
        assert_eq!(sorted[1][0], "2");
        assert_eq!(sorted[2][0], "3");
    }

    #[test]
    fn find_rows_returns_matching_indices() {
        let spec = TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("V", "value")]);
        let table = Table::new(
            spec,
            vec![
                vec!["1".to_string()],
                vec!["2".to_string()],
                vec!["30".to_string()],
            ],
        );
        assert_eq!(table.find_rows("V", "3"), vec![2]);
        assert_eq!(table.find_rows("V", ""), vec![0, 1, 2]);
    }

    #[test]
    fn filter_rows_keeps_only_matching_rows() {
        let spec = TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("V", "value")]);
        let table = Table::new(
            spec,
            vec![
                vec!["1".to_string()],
                vec!["2".to_string()],
                vec!["3".to_string()],
            ],
        );
        let kept = table.filter_rows("V", |v| v.parse::<f64>().map(|x| x > 1.5).unwrap_or(false));
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0][0], "2");
        assert_eq!(kept[1][0], "3");
    }

    #[test]
    fn selected_rows_returns_only_the_selected_rows() {
        let spec = TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("V", "value")]);
        let table = Table::new(
            spec,
            vec![
                vec!["a".to_string(), "1".to_string()],
                vec!["b".to_string(), "2".to_string()],
                vec!["c".to_string(), "3".to_string()],
            ],
        );
        assert_eq!(table.selected_rows(&[2, 0]).len(), 2);
        assert_eq!(table.selected_rows(&[2, 0])[0][0], "c");
        assert_eq!(table.selected_rows(&[2, 0])[1][0], "a");
        assert!(table.selected_rows(&[5]).is_empty());
    }

    #[test]
    fn column_format_kinds_format_calculated_values() {
        let plain = ColumnSpec::new("V", "value");
        assert_eq!(plain.format_value(2.0), "2");
        assert_eq!(plain.format_value(2.5), "2.5");

        let mut percent = ColumnSpec::formula("P", "percent", "0.25");
        percent.format = ColumnFormat::Percent;
        assert_eq!(percent.format_value(0.25), "25.00%");

        let mut sci = ColumnSpec::formula("S", "sci", "0.0015");
        sci.format = ColumnFormat::Scientific;
        assert_eq!(sci.format_value(0.0015), "1.500e-3");

        let mut text = ColumnSpec::formula("T", "text", "0");
        text.format = ColumnFormat::Text;
        assert_eq!(text.format_value(3.0), "3");
    }

    #[test]
    fn table_to_csv_writes_header_and_rows() {
        let table = Table::new(
            TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("V", "value")]),
            vec![vec!["1".to_string()], vec!["2".to_string()]],
        );
        assert_eq!(table_to_csv(&table), "value\n1\n2\n");
    }

    #[test]
    fn table_to_csv_empty_table_writes_nothing() {
        let table = Table::new(TableSpec::new(RowAxis::Samples, vec![]), vec![]);
        assert_eq!(table_to_csv(&table), "");
    }

    #[test]
    fn live_table_from_profile_builds_rows_per_statistic() {
        let profile = PopulationProfile {
            name: "test".into(),
            count: 10,
            parent_count: None,
            total_count: None,
            stats: vec![PopulationStat {
                parameter: "FITC".into(),
                statistic: Statistic::Mean,
                value: 5.0,
            }],
        };
        let table = Table::from_profile(&profile, vec![ColumnSpec::new("FITC", "FITC")]);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][0], "5");
    }

    #[test]
    fn live_table_from_profile_matches_statistic_label_columns() {
        let profile = PopulationProfile {
            name: "test".into(),
            count: 3,
            parent_count: None,
            total_count: None,
            stats: vec![PopulationStat {
                parameter: "FITC".into(),
                statistic: Statistic::Median,
                value: 7.5,
            }],
        };
        let table = Table::from_profile(&profile, vec![ColumnSpec::new("median", "median")]);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][0], "7.5");
    }

    #[test]
    fn table_to_html_escapes_and_renders_rows() {
        let table = Table::new(
            TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("V", "value")]),
            vec![vec!["1".to_string()], vec!["<b>2</b>".to_string()]],
        );
        let html = table_to_html(&table);
        assert!(html.contains("<th>value</th>"));
        assert!(html.contains("<td>1</td>"));
        assert!(html.contains("<td>&lt;b&gt;2&lt;/b&gt;</td>"));
    }

    #[test]
    fn table_to_rtf_starts_with_rtf_header() {
        let table = Table::new(
            TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("V", "value")]),
            vec![vec!["1".to_string()]],
        );
        assert!(table_to_rtf(&table).starts_with(r"{\rtf1"));
    }

    #[test]
    fn table_to_xml_escapes_and_renders_rows() {
        let table = Table::new(
            TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("V", "value")]),
            vec![vec!["1".to_string()]],
        );
        let xml = table_to_xml(&table);
        assert!(xml.contains("<table>"));
        assert!(xml.contains("<cell>1</cell>"));
    }

    #[test]
    fn table_to_sql_escapes_and_renders_create_and_inserts() {
        let table = Table::new(
            TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("V", "value")]),
            vec![vec!["1".to_string()], vec!["it's <5>".to_string()]],
        );
        let sql = table_to_sql(&table);
        assert!(sql.contains("CREATE TABLE flowers_table (value);"));
        assert!(sql.contains("INSERT INTO flowers_table (value) VALUES ('1');"));
        assert!(sql.contains("INSERT INTO flowers_table (value) VALUES ('it''s <5>');"));
        assert!(sql.contains("INSERT INTO flowers_table (value) VALUES ('1');\n"));
    }

    #[test]
    fn table_to_sql_without_columns_returns_empty() {
        let table = Table::new(TableSpec::new(RowAxis::Samples, vec![]), vec![]);
        assert_eq!(table_to_sql(&table), "");
    }

    #[test]
    fn table_to_xlsx_returns_a_valid_zip_workbook() {
        let table = Table::new(
            TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("V", "value")]),
            vec![vec!["1".to_string()]],
        );
        let bytes = table_to_xlsx(&table).unwrap();
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[0..2], b"PK");
    }

    #[test]
    fn table_editor_resolves_sample_population_and_statistic_columns() {
        let mut editors = TableEditor::new(RowAxis::Statistics);
        editors
            .add_source("sample", "Sample", ColumnSource::SampleId)
            .add_source("population", "Population", ColumnSource::PopulationName)
            .add_source(
                "fitc",
                "FITC mean",
                ColumnSource::Statistic {
                    parameter: "FITC".into(),
                    statistic: "mean".into(),
                },
            );
        let mut row = TableRow::sample("s1");
        row.population_name = "Lymphocytes".into();
        row.statistics = vec![PopulationStat {
            parameter: "FITC".into(),
            statistic: Statistic::Mean,
            value: 42.5,
        }];
        let table = editors.build(&[row]);
        assert_eq!(table.spec.rows, RowAxis::Statistics);
        assert_eq!(table.rows[0], vec!["s1", "Lymphocytes", "42.5"]);
    }

    #[test]
    fn table_editor_resolves_keywords_metadata_and_file_fields() {
        let mut keywords = std::collections::HashMap::new();
        keywords.insert("$TOT".to_string(), "10000".to_string());
        keywords.insert("$FIL".to_string(), "sample.fcs".to_string());
        keywords.insert("$WELL".to_string(), "B3".to_string());
        keywords.insert("$DATE".to_string(), "14-NOV-2026".to_string());
        keywords.insert("$BTIM".to_string(), "10:11:12".to_string());
        keywords.insert("$STATE".to_string(), "OK".to_string());
        keywords.insert("$REAGENT".to_string(), "CD3".to_string());
        keywords.insert("PATIENT".to_string(), "P-01".to_string());

        let mut editor = TableEditor::new(RowAxis::Samples);
        editor
            .add_source("tot", "Events", ColumnSource::TotalEvents)
            .add_source("file", "File", ColumnSource::FileUri)
            .add_source("well", "Well", ColumnSource::WellId)
            .add_source("time", "Collected", ColumnSource::CollectionTime)
            .add_source("state", "State", ColumnSource::SampleState)
            .add_source("reagent", "Reagent", ColumnSource::Reagent)
            .add_source(
                "patient",
                "Patient",
                ColumnSource::Keyword {
                    name: "patient".into(),
                },
            )
            .add_source(
                "meta",
                "Meta",
                ColumnSource::Metadata {
                    name: "$FIL".into(),
                },
            );
        let mut row = TableRow::sample("s1");
        row.keywords = keywords;
        let table = editor.build(&[row]);
        assert_eq!(
            table.rows[0],
            vec![
                "10000",
                "sample.fcs",
                "B3",
                "14-NOV-2026 10:11:12",
                "OK",
                "CD3",
                "P-01",
                "sample.fcs"
            ]
        );
    }

    #[test]
    fn table_editor_resolves_gate_compensation_transform_and_graph_columns() {
        let mut editor = TableEditor::new(RowAxis::Populations);
        editor
            .add_source(
                "gx",
                "X gate",
                ColumnSource::GateCoordinate { axis: GateAxis::X },
            )
            .add_source(
                "gy",
                "Y gate",
                ColumnSource::GateCoordinate { axis: GateAxis::Y },
            )
            .add_source(
                "ox",
                "X offset",
                ColumnSource::GateOffset { axis: GateAxis::X },
            )
            .add_source("comp", "Compensation", ColumnSource::CompensationMatrix)
            .add_source(
                "xform",
                "X transform",
                ColumnSource::TransformSetting {
                    setting: "x".into(),
                },
            )
            .add_source("graph", "Graph", ColumnSource::GraphSpecification)
            .add_source("quality", "Quality", ColumnSource::SampleQuality);
        let mut row = TableRow::sample("s1");
        row.gate_low = Some([10.0, 20.0]);
        row.gate_high = Some([30.0, 80.0]);
        row.compensation = Some("matrix-3".into());
        row.transform = Some("x=linear;y=logicle".into());
        row.graph_spec = Some("dot:FSC-A/SSC-A".into());
        row.quality = Some("pass".into());
        let table = editor.build(&[row]);
        assert_eq!(
            table.rows[0],
            vec![
                "10 - 30",
                "20 - 80",
                "20",
                "matrix-3",
                "linear",
                "dot:FSC-A/SSC-A",
                "pass"
            ]
        );
    }

    #[test]
    fn table_editor_adds_removes_moves_and_mutates_rows_and_columns() {
        let mut editor = TableEditor::new(RowAxis::Samples);
        editor
            .add_column(ColumnSpec::new("A", "Alpha"))
            .add_column(ColumnSpec::new("B", "Beta"))
            .add_column(ColumnSpec::new("C", "Gamma"));
        assert_eq!(editor.column_ids(), vec!["A", "B", "C"]);
        assert!(editor.move_column(2, 0));
        assert_eq!(editor.column_ids(), vec!["C", "A", "B"]);
        assert!(editor.remove_column("A"));
        assert_eq!(editor.column_ids(), vec!["C", "B"]);
        assert!(!editor.remove_column("missing"));
        assert!(!editor.move_column(9, 0));

        let mut table = editor.build(&[TableRow::sample("s1")]);
        assert_eq!(table.row_count(), 1);
        table.insert_row(0, vec!["c".into(), "b".into()]);
        assert_eq!(table.row_count(), 2);
        assert_eq!(table.rows[0], vec!["c", "b"]);
        table.insert_column(1, ColumnSpec::new("D", "Delta"));
        assert_eq!(table.column_count(), 3);
        assert_eq!(table.rows[0], vec!["c", "", "b"]);
        assert!(table.delete_column(2));
        assert_eq!(table.rows[0], vec!["c", ""]);
        assert!(table.delete_row(0));
        assert_eq!(table.row_count(), 1);
        assert!(!table.delete_row(5));
        assert!(!table.delete_column(5));
    }

    #[test]
    fn table_editor_sources_and_row_axes_round_trip_through_serde() {
        let editor = TableEditor::with_columns(
            RowAxis::Keywords,
            vec![ColumnSpec::sourced("well", "Well", ColumnSource::WellId)],
        );
        let json = serde_json::to_string(&editor).unwrap();
        let back: TableEditor = serde_json::from_str(&json).unwrap();
        assert_eq!(back, editor);
        assert_eq!(back.rows, RowAxis::Keywords);

        let stats = TableEditor::new(RowAxis::Statistics);
        assert_eq!(stats.rows, RowAxis::Statistics);
    }

    #[test]
    fn live_table_recalculates_from_new_rows() {
        let mut editor = TableEditor::new(RowAxis::Samples);
        editor.add_source("sample", "Sample", ColumnSource::SampleId);
        let mut live = LiveTable::new(editor);
        assert_eq!(live.table().row_count(), 0);

        live.recalculate(&[TableRow::sample("s1")]);
        assert_eq!(live.table().row_count(), 1);
        assert_eq!(live.table().rows[0][0], "s1");

        live.recalculate(&[TableRow::sample("s1"), TableRow::sample("s2")]);
        assert_eq!(live.table().row_count(), 2);
        assert_eq!(live.table().rows[1][0], "s2");
    }

    #[test]
    fn live_table_set_recalculates_every_table() {
        let mut first = TableEditor::new(RowAxis::Samples);
        first.add_source("sample", "Sample", ColumnSource::SampleId);
        let mut second = TableEditor::new(RowAxis::Populations);
        second.add_source("population", "Population", ColumnSource::PopulationName);

        let mut set = LiveTableSet::new();
        set.add(LiveTable::new(first)).add(LiveTable::new(second));
        assert_eq!(set.len(), 2);
        assert!(!set.is_empty());

        let mut row = TableRow::sample("s1");
        row.population_name = "Live".into();
        set.recalculate_all(&[row]);
        assert_eq!(set.tables[0].table().rows[0][0], "s1");
        assert_eq!(set.tables[1].table().rows[0][0], "Live");
    }

    #[test]
    fn table_templates_save_apply_replace_and_round_trip() {
        let mut editor = TableEditor::new(RowAxis::Samples);
        editor
            .add_source("tot", "Events", ColumnSource::TotalEvents)
            .add_source("file", "File", ColumnSource::FileUri);
        let template = TableTemplate::from_editor("metadata", &editor);
        let mut registry = TableTemplateRegistry::new();
        registry.save(template);
        assert_eq!(registry.names(), vec!["metadata"]);

        let mut keywords = std::collections::HashMap::new();
        keywords.insert("$TOT".to_string(), "7".to_string());
        keywords.insert("$FIL".to_string(), "x.fcs".to_string());
        let mut row = TableRow::sample("s1");
        row.keywords = keywords;
        let table = registry.apply("metadata", &[row.clone()]).unwrap();
        assert_eq!(table.rows[0], vec!["7", "x.fcs"]);
        assert!(registry.apply("missing", &[]).is_none());

        // Replace by name.
        editor.remove_column("file");
        registry.save(TableTemplate::from_editor("metadata", &editor));
        assert_eq!(registry.templates.len(), 1);
        assert_eq!(registry.get("metadata").unwrap().spec.columns.len(), 1);

        let json = serde_json::to_string(&registry).unwrap();
        let back: TableTemplateRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, registry);
        assert!(back.get("metadata").is_some());
        assert!(registry.remove("metadata"));
        assert!(!registry.remove("metadata"));
    }

    #[test]
    fn batch_export_writes_files_and_generates_documents() {
        let table = Table::new(
            TableSpec::new(RowAxis::Samples, vec![ColumnSpec::new("V", "value")]),
            vec![vec!["1".to_string()], vec!["2".to_string()]],
        );
        let csv_path =
            std::env::temp_dir().join(format!("flowers-table-{}-tb3.csv", std::process::id()));
        let html_path =
            std::env::temp_dir().join(format!("flowers-table-{}-tb3.html", std::process::id()));
        let outputs = export_table_batch(
            &table,
            &[
                BatchTableTarget::File(csv_path.clone()),
                BatchTableTarget::File(html_path.clone()),
                BatchTableTarget::Layout,
                BatchTableTarget::AnnotationFigure("Figure 1".into()),
                BatchTableTarget::Printer,
            ],
        )
        .unwrap();
        assert_eq!(outputs.len(), 5);
        assert!(outputs[0].written);
        assert_eq!(std::fs::read_to_string(&csv_path).unwrap(), "value\n1\n2\n");
        assert!(
            std::fs::read_to_string(&html_path)
                .unwrap()
                .contains("<td>1</td>")
        );
        assert!(outputs[2].content.starts_with("<svg"));
        assert!(outputs[3].content.contains("Figure 1"));
        assert!(outputs[4].content.starts_with("<!doctype html>"));
        assert!(outputs[4].content.contains("@media print"));
        let _ = std::fs::remove_file(csv_path);
        let _ = std::fs::remove_file(html_path);
    }

    #[test]
    fn table_to_svg_and_layout_object_render_cells() {
        let table = Table::new(
            TableSpec::new(
                RowAxis::Samples,
                vec![ColumnSpec::new("V", "value"), ColumnSpec::new("N", "name")],
            ),
            vec![vec!["1".to_string(), "a<b".to_string()]],
        );
        let svg = table_to_svg(&table, "My table");
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("My table"));
        assert!(svg.contains(">value</text>"));
        assert!(svg.contains(">1</text>"));
        assert!(svg.contains("a&lt;b"));

        let object = table_layout_object(&table, "My table", 10.0, 20.0, 300.0);
        match object {
            crate::layout::LayoutObject::Table(layout) => {
                assert_eq!(layout.title, "My table");
                assert_eq!(layout.columns, vec!["value", "name"]);
                assert_eq!(layout.rows.len(), 1);
                assert_eq!(layout.x, 10.0);
                assert_eq!(layout.y, 20.0);
                assert_eq!(layout.width, 300.0);
            }
            other => panic!("expected table layout object, got {other:?}"),
        }
    }
}
