//! Plate editor (Phase 9, W3).
//!
//! Maps samples to wells on 96/384-well (or custom) plates, applies reusable
//! plate templates, assigns groups and titration series, and stores per-well
//! metadata.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Supported plate formats (W3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlateFormat {
    /// 8 rows x 12 columns.
    Wells96,
    /// 16 rows x 24 columns.
    Wells384,
    /// Arbitrary plate dimensions.
    Custom { rows: usize, columns: usize },
}

impl PlateFormat {
    pub fn custom(rows: usize, columns: usize) -> Self {
        Self::Custom {
            rows: rows.max(1),
            columns: columns.max(1),
        }
    }

    pub fn rows(self) -> usize {
        match self {
            Self::Wells96 => 8,
            Self::Wells384 => 16,
            Self::Custom { rows, .. } => rows.max(1),
        }
    }

    pub fn columns(self) -> usize {
        match self {
            Self::Wells96 => 12,
            Self::Wells384 => 24,
            Self::Custom { columns, .. } => columns.max(1),
        }
    }

    pub fn well_count(self) -> usize {
        self.rows() * self.columns()
    }

    /// Well label for zero-based row/column coordinates.
    pub fn well_label(self, row: usize, column: usize) -> Option<String> {
        if row >= self.rows() || column >= self.columns() {
            return None;
        }
        Some(WellId::new(row, column).label())
    }
}

/// Zero-based well coordinates (W3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WellId {
    pub row: usize,
    pub column: usize,
}

impl WellId {
    pub fn new(row: usize, column: usize) -> Self {
        Self { row, column }
    }

    /// Spreadsheet-style label (`A1`, `B12`, `AA3`).
    pub fn label(&self) -> String {
        format!("{}{}", row_label(self.row), self.column + 1)
    }

    /// Parse a label such as `A1` or `AA12` into zero-based coordinates.
    pub fn from_label(label: &str) -> Option<Self> {
        let label = label.trim();
        let digit = label.find(|character: char| character.is_ascii_digit())?;
        let (letters, digits) = label.split_at(digit);
        if letters.is_empty() || digits.is_empty() {
            return None;
        }
        let mut row = 0usize;
        for character in letters.chars() {
            if !character.is_ascii_alphabetic() {
                return None;
            }
            row = row
                .checked_mul(26)?
                .checked_add((character.to_ascii_uppercase() as u8 - b'A') as usize + 1)?;
        }
        let column: usize = digits.parse().ok()?;
        if row == 0 || column == 0 {
            return None;
        }
        Some(Self {
            row: row - 1,
            column: column - 1,
        })
    }
}

fn row_label(mut row: usize) -> String {
    let mut letters = Vec::new();
    loop {
        letters.push((b'A' + (row % 26) as u8) as char);
        if row < 26 {
            break;
        }
        row = row / 26 - 1;
    }
    letters.iter().rev().collect()
}

/// Metadata stored on one well (W3).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WellMetadata {
    /// Sample mapped to this well.
    pub sample_id: Option<String>,
    /// Group assignment.
    pub group: Option<String>,
    /// Titration/series name.
    pub titration: Option<String>,
    /// Titration concentration.
    pub concentration: Option<f64>,
    /// Replicate index (0-based or user-defined).
    pub replicate: Option<u32>,
    /// Display label.
    pub label: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// Arbitrary well keywords.
    pub keywords: BTreeMap<String, String>,
}

impl WellMetadata {
    pub fn is_empty(&self) -> bool {
        self.sample_id.is_none()
            && self.group.is_none()
            && self.titration.is_none()
            && self.concentration.is_none()
            && self.replicate.is_none()
            && self.label.is_none()
            && self.notes.is_none()
            && self.keywords.is_empty()
    }

    pub fn for_sample(sample_id: impl Into<String>) -> Self {
        Self {
            sample_id: Some(sample_id.into()),
            ..Self::default()
        }
    }

    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_replicate(mut self, replicate: u32) -> Self {
        self.replicate = Some(replicate);
        self
    }

    pub fn with_notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = Some(notes.into());
        self
    }

    pub fn with_keyword(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.keywords.insert(key.into(), value.into());
        self
    }
}

/// One concentration level of a titration series (W3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TitrationLevel {
    pub concentration: f64,
    pub label: Option<String>,
}

impl TitrationLevel {
    pub fn new(concentration: f64) -> Self {
        Self {
            concentration,
            label: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

/// A named titration series (W3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Titration {
    pub name: String,
    /// Levels in assignment order (typically high to low concentration).
    pub levels: Vec<TitrationLevel>,
}

impl Titration {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            levels: Vec::new(),
        }
    }

    pub fn from_concentrations(name: impl Into<String>, concentrations: &[f64]) -> Self {
        Self {
            name: name.into(),
            levels: concentrations
                .iter()
                .map(|concentration| TitrationLevel::new(*concentration))
                .collect(),
        }
    }

    pub fn with_level(mut self, concentration: f64, label: impl Into<String>) -> Self {
        self.levels
            .push(TitrationLevel::new(concentration).with_label(label));
        self
    }

    pub fn add_level(&mut self, concentration: f64) -> usize {
        self.levels.push(TitrationLevel::new(concentration));
        self.levels.len() - 1
    }

    pub fn level(&self, index: usize) -> Option<&TitrationLevel> {
        self.levels.get(index)
    }

    pub fn len(&self) -> usize {
        self.levels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }
}

/// A reusable plate template (W3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlateTemplate {
    pub name: String,
    pub format: PlateFormat,
    /// Defaults applied to every well when the template is instantiated.
    pub defaults: WellMetadata,
    /// Suggested group names.
    pub groups: Vec<String>,
}

impl PlateTemplate {
    pub fn new(name: impl Into<String>, format: PlateFormat) -> Self {
        Self {
            name: name.into(),
            format,
            defaults: WellMetadata::default(),
            groups: Vec::new(),
        }
    }

    pub fn with_defaults(mut self, defaults: WellMetadata) -> Self {
        self.defaults = defaults;
        self
    }

    pub fn with_default_group(mut self, group: impl Into<String>) -> Self {
        self.defaults.group = Some(group.into());
        self
    }

    pub fn with_default_keyword(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.defaults.keywords.insert(key.into(), value.into());
        self
    }

    pub fn with_groups(mut self, groups: Vec<String>) -> Self {
        self.groups = groups;
        self
    }

    /// Instantiate a plate, applying template defaults to every well (W3).
    pub fn instantiate(&self, name: impl Into<String>) -> Plate {
        let mut plate = Plate::new(name, self.format);
        plate.groups = self.groups.clone();
        if !self.defaults.is_empty() {
            for well in &mut plate.wells {
                *well = Some(self.defaults.clone());
            }
        }
        plate
    }
}

/// Registry of reusable plate templates (W3).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PlateTemplateRegistry {
    pub templates: Vec<PlateTemplate>,
}

impl PlateTemplateRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace a template by name; returns its index.
    pub fn add(&mut self, template: PlateTemplate) -> usize {
        if let Some(index) = self
            .templates
            .iter()
            .position(|existing| existing.name == template.name)
        {
            self.templates[index] = template;
            index
        } else {
            self.templates.push(template);
            self.templates.len() - 1
        }
    }

    pub fn get(&self, name: &str) -> Option<&PlateTemplate> {
        self.templates.iter().find(|template| template.name == name)
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.templates.len();
        self.templates.retain(|template| template.name != name);
        self.templates.len() != before
    }

    pub fn names(&self) -> Vec<&str> {
        self.templates
            .iter()
            .map(|template| template.name.as_str())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    /// Instantiate a template by name.
    pub fn instantiate(&self, template: &str, plate: impl Into<String>) -> Option<Plate> {
        Some(self.get(template)?.instantiate(plate))
    }
}

/// A plate: format plus per-well metadata, groups and titration definitions
/// (W3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plate {
    pub name: String,
    pub format: PlateFormat,
    /// Row-major well metadata.
    pub wells: Vec<Option<WellMetadata>>,
    /// Group names available on this plate.
    pub groups: Vec<String>,
    /// Titration series defined on this plate.
    pub titrations: Vec<Titration>,
}

impl Plate {
    pub fn new(name: impl Into<String>, format: PlateFormat) -> Self {
        Self {
            name: name.into(),
            format,
            wells: vec![None; format.well_count()],
            groups: Vec::new(),
            titrations: Vec::new(),
        }
    }

    pub fn from_template(name: impl Into<String>, template: &PlateTemplate) -> Self {
        template.instantiate(name)
    }

    fn index(&self, row: usize, column: usize) -> Option<usize> {
        if row >= self.format.rows() || column >= self.format.columns() {
            return None;
        }
        Some(row * self.format.columns() + column)
    }

    /// Read a well's metadata.
    pub fn well(&self, row: usize, column: usize) -> Option<&WellMetadata> {
        self.wells.get(self.index(row, column)?)?.as_ref()
    }

    /// Get or create a well's metadata.
    pub fn metadata_mut(&mut self, row: usize, column: usize) -> Option<&mut WellMetadata> {
        let index = self.index(row, column)?;
        Some(self.wells[index].get_or_insert_with(WellMetadata::default))
    }

    /// Replace a well's metadata.
    pub fn set_well(&mut self, row: usize, column: usize, metadata: WellMetadata) -> bool {
        let Some(index) = self.index(row, column) else {
            return false;
        };
        self.wells[index] = Some(metadata);
        true
    }

    /// Read a well by label (`A1`).
    pub fn well_by_label(&self, label: &str) -> Option<&WellMetadata> {
        let well = WellId::from_label(label)?;
        self.well(well.row, well.column)
    }

    /// Remove a well's metadata.
    pub fn clear_well(&mut self, row: usize, column: usize) -> bool {
        let Some(index) = self.index(row, column) else {
            return false;
        };
        self.wells[index] = None;
        true
    }

    /// Map a sample to one well.
    pub fn assign_sample(
        &mut self,
        row: usize,
        column: usize,
        sample_id: impl Into<String>,
    ) -> bool {
        let Some(well) = self.metadata_mut(row, column) else {
            return false;
        };
        well.sample_id = Some(sample_id.into());
        true
    }

    /// Map a sample to several wells (replicates).
    pub fn assign_sample_to_wells(&mut self, sample_id: &str, wells: &[(usize, usize)]) -> usize {
        wells
            .iter()
            .filter(|(row, column)| self.assign_sample(*row, *column, sample_id))
            .count()
    }

    pub fn add_group(&mut self, group: impl Into<String>) -> usize {
        let group = group.into();
        if let Some(index) = self.groups.iter().position(|existing| *existing == group) {
            return index;
        }
        self.groups.push(group);
        self.groups.len() - 1
    }

    /// Assign a group to one well.
    pub fn assign_group(&mut self, row: usize, column: usize, group: impl Into<String>) -> bool {
        let group = group.into();
        if !self.groups.contains(&group) {
            self.groups.push(group.clone());
        }
        let Some(well) = self.metadata_mut(row, column) else {
            return false;
        };
        well.group = Some(group);
        true
    }

    /// Assign a group to a rectangular well range (end exclusive).
    pub fn assign_group_range(
        &mut self,
        rows: std::ops::Range<usize>,
        columns: std::ops::Range<usize>,
        group: impl Into<String>,
    ) -> usize {
        let group = group.into();
        let mut assigned = 0;
        for row in rows {
            for column in columns.clone() {
                if self.assign_group(row, column, group.clone()) {
                    assigned += 1;
                }
            }
        }
        assigned
    }

    /// Assign a whole column to one group.
    pub fn assign_group_column(&mut self, column: usize, group: impl Into<String>) -> usize {
        self.assign_group_range(0..self.format.rows(), column..column + 1, group)
    }

    pub fn add_titration(&mut self, titration: Titration) -> usize {
        if let Some(index) = self
            .titrations
            .iter()
            .position(|existing| existing.name == titration.name)
        {
            self.titrations[index] = titration;
            return index;
        }
        self.titrations.push(titration);
        self.titrations.len() - 1
    }

    pub fn titration(&self, name: &str) -> Option<&Titration> {
        self.titrations
            .iter()
            .find(|titration| titration.name == name)
    }

    /// Assign one titration level to a well.
    pub fn assign_titration(
        &mut self,
        row: usize,
        column: usize,
        titration: &Titration,
        level: usize,
    ) -> bool {
        let Some(level) = titration.level(level) else {
            return false;
        };
        let label = level
            .label
            .clone()
            .unwrap_or_else(|| format!("{}", level.concentration));
        let Some(well) = self.metadata_mut(row, column) else {
            return false;
        };
        well.titration = Some(titration.name.clone());
        well.concentration = Some(level.concentration);
        if well.label.is_none() {
            well.label = Some(label);
        }
        true
    }

    /// Assign a titration series down a column, starting at `start_row`.
    pub fn assign_titration_column(
        &mut self,
        start_row: usize,
        column: usize,
        titration: &Titration,
    ) -> usize {
        let mut assigned = 0;
        for (offset, _) in titration.levels.iter().enumerate() {
            if !self.assign_titration(start_row + offset, column, titration, offset) {
                break;
            }
            assigned += 1;
        }
        assigned
    }

    /// Assign a titration series across a row, starting at `start_column`.
    pub fn assign_titration_row(
        &mut self,
        row: usize,
        start_column: usize,
        titration: &Titration,
    ) -> usize {
        let mut assigned = 0;
        for (offset, _) in titration.levels.iter().enumerate() {
            if !self.assign_titration(row, start_column + offset, titration, offset) {
                break;
            }
            assigned += 1;
        }
        assigned
    }

    /// Wells mapped to a sample.
    pub fn wells_for_sample(&self, sample_id: &str) -> Vec<WellId> {
        self.occupied_wells()
            .filter(|(_, metadata)| metadata.sample_id.as_deref() == Some(sample_id))
            .map(|(well, _)| well)
            .collect()
    }

    /// Wells assigned to a group.
    pub fn wells_for_group(&self, group: &str) -> Vec<WellId> {
        self.occupied_wells()
            .filter(|(_, metadata)| metadata.group.as_deref() == Some(group))
            .map(|(well, _)| well)
            .collect()
    }

    /// Wells assigned to a titration series.
    pub fn wells_for_titration(&self, titration: &str) -> Vec<WellId> {
        self.occupied_wells()
            .filter(|(_, metadata)| metadata.titration.as_deref() == Some(titration))
            .map(|(well, _)| well)
            .collect()
    }

    /// Iterate occupied wells in row-major order together with their metadata.
    pub fn occupied_wells(&self) -> impl Iterator<Item = (WellId, &WellMetadata)> {
        let columns = self.format.columns();
        self.wells
            .iter()
            .enumerate()
            .filter_map(move |(index, metadata)| {
                let metadata = metadata.as_ref()?;
                Some((WellId::new(index / columns, index % columns), metadata))
            })
    }

    /// Number of wells carrying metadata.
    pub fn occupied_count(&self) -> usize {
        self.wells.iter().filter(|well| well.is_some()).count()
    }

    /// Export the plate as CSV (W3).
    pub fn to_csv(&self) -> String {
        let mut out = String::from(
            "Plate,Well,Row,Column,Sample,Group,Titration,Concentration,Replicate,Label,Notes,Keywords\n",
        );
        let columns = self.format.columns();
        for (index, metadata) in self.wells.iter().enumerate() {
            let Some(metadata) = metadata else {
                continue;
            };
            let row = index / columns;
            let column = index % columns;
            let well = WellId::new(row, column);
            let keywords = metadata
                .keywords
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(";");
            let concentration = metadata
                .concentration
                .map(|value| format!("{value}"))
                .unwrap_or_default();
            let replicate = metadata
                .replicate
                .map(|value| value.to_string())
                .unwrap_or_default();
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{},{}\n",
                csv_field(&self.name),
                well.label(),
                row + 1,
                column + 1,
                csv_field(metadata.sample_id.as_deref().unwrap_or("")),
                csv_field(metadata.group.as_deref().unwrap_or("")),
                csv_field(metadata.titration.as_deref().unwrap_or("")),
                concentration,
                replicate,
                csv_field(metadata.label.as_deref().unwrap_or("")),
                csv_field(metadata.notes.as_deref().unwrap_or("")),
                csv_field(&keywords),
            ));
        }
        out
    }
}

fn csv_field(text: &str) -> String {
    if text.contains(',') || text.contains('"') || text.contains('\n') || text.contains('\r') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_well_labels_round_trip() {
        assert_eq!(PlateFormat::Wells96.rows(), 8);
        assert_eq!(PlateFormat::Wells96.columns(), 12);
        assert_eq!(PlateFormat::Wells96.well_count(), 96);
        assert_eq!(PlateFormat::Wells384.rows(), 16);
        assert_eq!(PlateFormat::Wells384.well_count(), 384);
        assert_eq!(PlateFormat::Wells96.well_label(0, 0).as_deref(), Some("A1"));
        assert_eq!(
            PlateFormat::Wells96.well_label(7, 11).as_deref(),
            Some("H12")
        );
        assert_eq!(
            PlateFormat::Wells384.well_label(15, 23).as_deref(),
            Some("P24")
        );
        assert_eq!(PlateFormat::Wells96.well_label(8, 0), None);

        for label in ["A1", "B12", "H12", "AA1", "P24"] {
            let well = WellId::from_label(label).expect("well label");
            assert_eq!(well.label(), label);
        }
        assert_eq!(WellId::from_label("aa3"), Some(WellId::new(26, 2)));
        assert_eq!(WellId::from_label("1A"), None);
        assert_eq!(WellId::from_label("A0"), None);
    }

    #[test]
    fn templates_instantiate_defaults_and_samples() {
        let template = PlateTemplate::new("96-well U-bottom", PlateFormat::Wells96)
            .with_default_group("Control")
            .with_default_keyword("Medium", "RPMI")
            .with_groups(vec!["Control".to_string(), "Treated".to_string()]);
        let mut plate = template.instantiate("Experiment 1");
        assert_eq!(plate.occupied_count(), 96);
        assert_eq!(plate.well(0, 0).unwrap().group.as_deref(), Some("Control"));
        assert_eq!(
            plate
                .well(0, 0)
                .unwrap()
                .keywords
                .get("Medium")
                .map(String::as_str),
            Some("RPMI")
        );
        assert_eq!(plate.groups, ["Control", "Treated"]);

        assert!(plate.assign_sample(0, 0, "S1"));
        assert!(plate.assign_sample(0, 1, "S1"));
        assert_eq!(plate.wells_for_sample("S1").len(), 2);
        assert_eq!(plate.wells_for_sample("S1")[0].label(), "A1");
        assert_eq!(
            plate.well_by_label("A1").unwrap().sample_id.as_deref(),
            Some("S1")
        );

        let registry = PlateTemplateRegistry {
            templates: vec![template],
        };
        assert!(registry.instantiate("96-well U-bottom", "Copy").is_some());
        assert!(registry.instantiate("missing", "Copy").is_none());
    }

    #[test]
    fn groups_and_titrations_assign_to_wells() {
        let mut plate = Plate::new("Titration", PlateFormat::Wells96);
        plate.add_group("Standard");
        assert_eq!(plate.assign_group_range(0..8, 0..1, "Standard"), 8);
        assert_eq!(plate.wells_for_group("Standard").len(), 8);

        let titration = Titration::new("Dose")
            .with_level(100.0, "100 ng")
            .with_level(50.0, "50 ng")
            .with_level(25.0, "25 ng");
        plate.add_titration(titration.clone());
        assert_eq!(plate.assign_titration_column(0, 1, &titration), 3);
        assert_eq!(plate.wells_for_titration("Dose").len(), 3);
        assert_eq!(plate.well(0, 1).unwrap().concentration, Some(100.0));
        assert_eq!(plate.well(1, 1).unwrap().label.as_deref(), Some("50 ng"));
        assert_eq!(plate.well(3, 1), None);

        let row_titration = Titration::from_concentrations("RowDose", &[10.0, 5.0]);
        assert_eq!(plate.assign_titration_row(2, 2, &row_titration), 2);
        assert_eq!(plate.well(2, 3).unwrap().concentration, Some(5.0));
        assert!(!plate.assign_titration(0, 0, &titration, 9));
    }

    #[test]
    fn well_metadata_and_csv_export() {
        let mut plate = Plate::new("Metadata, Plate", PlateFormat::Wells96);
        let metadata = WellMetadata::for_sample("S1")
            .with_group("Test")
            .with_replicate(2)
            .with_label("Sample A")
            .with_notes("first, replicate")
            .with_keyword("Operator", "AB");
        assert!(plate.set_well(0, 0, metadata));
        assert!(plate.clear_well(0, 0));
        assert_eq!(plate.well(0, 0), None);
        assert!(plate.set_well(1, 1, WellMetadata::for_sample("S2")));
        let csv = plate.to_csv();
        assert!(csv.starts_with("Plate,Well,Row,Column,Sample"));
        assert!(csv.contains("\"Metadata, Plate\",B2,2,2,S2"));
        assert!(!csv.contains("first, replicate"));
        assert_eq!(plate.occupied_count(), 1);
    }

    #[test]
    fn template_registry_replaces_and_serializes() {
        let mut registry = PlateTemplateRegistry::new();
        registry.add(PlateTemplate::new("A", PlateFormat::Wells96));
        registry.add(PlateTemplate::new("B", PlateFormat::Wells384));
        assert_eq!(registry.len(), 2);
        registry
            .add(PlateTemplate::new("A", PlateFormat::custom(4, 6)).with_default_group("Control"));
        assert_eq!(registry.len(), 2);
        assert_eq!(registry.get("A").unwrap().format.rows(), 4);
        let json = serde_json::to_string(&registry).expect("serialize registry");
        let restored: PlateTemplateRegistry =
            serde_json::from_str(&json).expect("deserialize registry");
        assert_eq!(restored, registry);
        assert!(registry.remove("B"));
        assert!(!registry.remove("B"));
        assert_eq!(registry.names(), ["A"]);
    }
}
