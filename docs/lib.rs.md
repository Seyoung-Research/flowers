# `lib.rs` Documentation

## High-Level Purpose
`src/lib.rs` serves as the crate root for the `flowers` library. Its primary responsibility is to define the library's module hierarchy and provide a clean, flat public API by re-exporting key types and functions from its internal modules.

The `flowers` library is designed as an open-source flow cytometry analysis tool, providing capabilities for parsing data, performing compensation, gating populations, calculating statistics, and visualizing results.

## Module Structure
The library is organized into several specialized modules:

| Module | Responsibility |
| :--- | :--- |
| `compensation` | Handles spillover correction and compensation logic. |
| `export` | Provides functionality to export population data to CSV format. |
| `fcs` | Implements parsing for FCS 2.0 and 3.0 files. |
| `gating` | Defines various gating strategies (rectangular, ellipse, polygon, etc.). |
| `gui` | Contains GUI-related components. |
| `layout` | Manages the arrangement and rendering of plot panels. |
| `plot` | Handles visualization logic, including density grids and plot styles. |
| `project` | Manages project-level data and persistence. |
| `raster` | Provides utilities to convert SVG visualizations into raster images. |
| `statistics` | Calculates statistical properties for defined populations. |
| `workspace` | Manages the overall analysis workspace and population hierarchies. |

## Public API (Re-exports)

### Compensation
- `CompensationEngine`: The core engine used to apply spillover correction to flow cytometry data.

### Export
- `populations_csv`: Function for generating CSV representations of populations.
- `write_populations_csv`: Function to write population data to a CSV file.
- `PopulationRow`: Data structure representing a single row in a population export.

### FCS Parsing
- `FcsError`: Error types associated with FCS file parsing.
- `FcsFile`: Representation of a parsed FCS file.
- `FcsParser`: The parser used to read FCS data.
- `Parameter`: Metadata describing a specific channel/parameter in an FCS file.

### Gating
The library provides a variety of gate types through the `Gate` trait:
- `BilinearGate`, `BooleanGate`, `EllipseGate`, `LineGate`, `PolygonGate`, `QuadrantGate`, `RectangularGate`: Specific implementations of gating logic.
- `Quadrant`: Helper type for quadrant-based gating.

### Layout & Visualization
- `render_layout`, `render_layout_free`: Functions to render the arrangement of plot panels.
- `LayoutOptions`, `LayoutPanel`, `LayoutPlacement`: Types defining how plots are positioned and configured.
- `density_grid`, `point_densities`: Utilities for calculating data density for heatmaps/plots.
- `AxisScale`, `PlotOverlay`, `PlotStyle`: Configuration types for plot axes and visual appearance.

### Rasterization
- `rasterize_svg`: Converts an SVG string/file into a raster image.
- `save_svg_as_image`: Utility to save an SVG directly to a file in a specified format.
- `ImageFormat`: Supported raster image formats.

### Statistics
- `PopulationStat`: Statistical results for a specific population.
- `Statistic`: Definition of a specific statistical measure to calculate.

### Workspace
- `Population`: Represents a subset of data defined by gates.
- `Workspace`: The top-level container managing the analysis state and population tree.

## Design Patterns
The library utilizes a **Facade Pattern** in `lib.rs`. Instead of requiring users to navigate deep module paths (e.g., `flowers::workspace::Workspace`), the crate root re-exports the most important types, allowing users to access them directly via `flowers::Workspace`.

## Relationships and Dependencies
`lib.rs` acts as the central hub. All other modules in the `src/` directory are declared here, and they depend on each other to form the full analysis pipeline:
`fcs` (Data Input) $\rightarrow$ `compensation` (Correction) $\rightarrow$ `gating` (Filtering) $\rightarrow$ `statistics` (Analysis) $\rightarrow$ `plot`/`layout`/`raster` (Visualization).
