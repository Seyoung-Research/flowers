# src/export.rs

## Overview
The `export` module provides utilities for generating CSV reports of gated population counts and percentages. It is primarily used to export flow cytometry population analysis results into a format suitable for spreadsheet software, allowing for the tracking of population sizes across multiple samples and gate hierarchies.

## Data Structures

### `PopulationRow`
A structure representing a single entry in a population report.

| Field | Type | Description |
|---|---|---|
| `sample` | `String` | The name of the sample (typically the source file). |
| `population` | `String` | The name of the population or gate. |
| `parent` | `Option<String>` | The name of the parent population in the gating hierarchy, if applicable. |
| `count` | `usize` | The number of events that fell within this population. |
| `total` | `usize` | The total number of events in the sample. |
| `parent_count` | `Option<usize>` | The number of events in the parent population, if applicable. |
| `gate_summary` | `Option<String>` | A description of the gate (e.g., geometry and coordinates). |

#### Methods
- `percent_of_total() -> f64`: Calculates the percentage of the whole sample that this population represents. Returns `0.0` if the total count is 0.
- `percent_of_parent() -> Option<f64>`: Calculates the percentage of the parent population that this population represents. Returns `None` if no parent count is provided; returns `0.0` if the parent count is 0.

## Functions

### `csv_field(s: &str) -> String`
A private helper function that ensures a string field is correctly escaped for CSV format. If the string contains commas, quotes, or newlines, it wraps the string in double quotes and escapes existing double quotes by doubling them.

### `populations_csv(rows: &[PopulationRow]) -> String`
Renders a slice of `PopulationRow` objects into a full CSV string.
- **Header**: `Sample,Population,Parent,Count,Total,PercentOfTotal,PercentOfParent,GateSummary`
- **Logic**: It iterates through the rows, calculating percentages on the fly and escaping all string fields using `csv_field`. Percentages are formatted to 4 decimal places.

### `write_populations_csv(path: &Path, rows: &[PopulationRow]) -> std::io::Result<()>`
Convenience function that takes a file path and a slice of population rows, generates the CSV string via `populations_csv`, and writes the resulting bytes to the specified file.

## Design Patterns & Implementation Details
- **Data-Driven Export**: The module separates the data representation (`PopulationRow`) from the formatting logic (`populations_csv`), allowing the data to be collected in one part of the application and exported in another.
- **Robust CSV Handling**: Rather than using a heavy CSV crate, it implements a lightweight `csv_field` escaping mechanism to handle common special characters in sample or population names.
- **Error Handling**: `write_populations_csv` bubbles up `std::io::Error` to the caller, ensuring that filesystem failures (like permission issues or full disks) are handled by the application's main error logic.

## Relationships
This module is a downstream consumer of the gating and analysis logic. It takes the final counts produced by the population analysis pipeline and transforms them into a persistent report.
