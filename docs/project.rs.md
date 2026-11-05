# src/project.rs

## Overview
The `project` module defines the data structures and serialization logic for GUI project/workspace files (saved with the `.wsp` extension). 

A project stores the **analysis state** rather than the raw data. It maintains references to FCS files via their file system paths, along with the gating strategies, view settings, and layout configurations. This design ensures that project files remains small, as large FCS files are stored externally and re-read upon loading the project.

The module is designed to be independent of the GUI framework (`egui`), serving as a set of plain data types that the GUI transforms into its internal state.

## Data Structures

### `Project`
The root structure representing a saved analysis.
- **Fields**:
    - `version`: The version of the project format (`PROJECT_VERSION`).
    - `samples`: A list of `ProjectSample` objects referenced by the project.
    - `current_sample`: Index of the currently active sample.
    - `x_param` / `y_param`: Indices of the parameters currently assigned to the X and Y axes.
    - `selected_gate`: Index of the currently selected gate.
    - `view`: Global `ViewSettings` for plots.
    - `gates`: A collection of all `ProjectGate` definitions.
    - `layout`: A list of `ProjectPanel` objects defining the workspace layout.
    - `layout_columns`: Number of columns used in the layout grid.

### `ProjectSample`
Represents a reference to an external FCS data file.
- **Fields**:
    - `path`: The file system path to the FCS file.
    - `apply_gates`: Boolean indicating if the shared gate set should be applied to this specific sample.

### `ViewSettings`
Controls the visual appearance and scaling of plots.
- **Fields**:
    - `mode`: The rendering mode (e.g., `"dots"`, `"contour"`, `"histogram"`).
    - `x_scale` / `y_scale`: Axis scaling mode (e.g., `"linear"`, `"log"`).
    - `point_color`: RGB color of the data points.
    - `gradient`: Whether density gradient layering is enabled.
    - `contour_levels`: Number of levels used for contour plots.
    - `x_range` / `y_range`: The visible coordinate range for the X and Y axes.

### `ProjectGate`
Defines a gating region and its metadata.
- **Fields**:
    - `name`: The descriptive name of the gate.
    - `x_idx` / `y_idx`: The parameters used to define the gate's boundaries.
    - `parent`: Optional index of the parent gate in the hierarchy.
    - `owner`: Optional index of the gate that "owns" this one.
    - `apply_to_all`: If true, this gate is applied across all samples.
    - `color`: RGB color for the gate boundary.
    - `kind`: The geometric shape of the gate (`GateShape`).

### `GateShape`
An enum defining the geometry of a gate:
- `Rect`: A rectangle defined by `x_min`, `x_max`, `y_min`, `y_max`, and an `angle` of rotation.
- `Ellipse`: An ellipse defined by `x_min`, `x_max`, `y_min`, `y_max`, and an `angle` of rotation.
- `Polygon`: An arbitrary shape defined by a list of `vertices` (coordinates).
- `Quadrant`: A quadrant defined by a center point (`x`, `y`) and a specific quadrant identifier (`quad`).
- `Bilinear`: A linear boundary defined by two points (`x1, y1` and `x2, y2`) and a `below` flag to determine the active side of the line.

### `ProjectPanel`
Defines a single plot panel within the layout editor.
- **Fields**:
    - `sample`: Index of the sample being plotted.
    - `x` / `y`: Indices of the parameters plotted on the axes.
    - `view`: The rendering mode for this specific panel.
    - `gate`: Optional index of a gate used to filter the view.
    - `title`: The label for the panel.
    - `title_size`: Font size of the title.
    - `rect`: The bounding box of the canvas `[x, y, w, h]`.
    - `x_label` / `y_label`: Styling and text for the axis labels.

### `ProjectLabel`
Styling information for axis labels.
- **Fields**:
    - `text`: The label text.
    - `size`: Font size.
    - `rotation_deg`: Rotation of the label in degrees.
    - `font`: The font family (e.g., `"sans"`, `"serif"`, `"mono"`).
    - `color`: RGB color of the text.

## Key Functions

### `to_json(project: &Project) -> Result<String, serde_json::Error>`
Serializes the `Project` structure into a pretty-printed JSON string for storage in a `.wsp` file.

### `from_json(text: &str) -> Result<Project, serde_json::Error>`
Parses a JSON string from a `.wsp` file back into a `Project` structure.

## Implementation Details & Design Patterns
- **Serialization**: The module relies heavily on `serde` for conversion between Rust types and JSON.
- **Backward Compatibility**: Most structures implement `Default` and use `#[serde(default)]` attributes. This allows the application to load older project files that may be missing newer fields by filling them with sensible defaults.
- **Decoupling**: By separating the project definition from the GUI state, the core logic remains testable and independent of the rendering loop.
- **Hierarchy**: Gates are organized hierarchically using indices (`parent`, `owner`), allowing for complex nested gating strategies.
