# `src/plot.rs`

Rendering helpers and density analysis for flow cytometry plots.

## High-Level Purpose

The `plot` module is responsible for converting raw event data (parameter values) into visual representations. It is divided into two primary functional areas:

1.  **SVG Export**: Provides high-level functions (`dot_plot_svg`, `histogram_svg`) that generate standalone SVG strings. This enables the application to export plots as files or run in headless environments (CLI) without requiring a graphics GPU.
2.  **Density Analysis**: Provides tools for computing 2D event densities, interpolating those densities for per-point coloring, and extracting topographic contour lines. These utilities are shared by both the SVG renderer and the GUI.

---

## Data Structures

### `AxisScale` (Enum)
Defines the mapping between data values and plot coordinates.
- **Variants**:
    - `Linear`: Standard linear mapping.
    - `Log`: Base-10 logarithmic mapping.
- **Key Methods**:
    - `transform(v: f64) -> Option<f64>`: Converts a data value to a plot position. Returns `None` for non-positive values on log axes.
    - `inverse(t: f64) -> f64`: Converts a plot position back to the original data value.
    - `label() -> &'static str`: Returns "Linear" or "Log".

### `PlotStyle` (Struct)
A configuration object that defines the visual appearance of a rendered plot.
- **Dimensions**: `width`, `height`.
- **Point Styling**: `point_radius`, `point_opacity`, `point_color`.
- **Axes & Text**: `line_color`, `axis_color`, `text_color`, `background`.
- **Scaling**: `x_scale`, `y_scale` (both `AxisScale`).
- **Visual Modes**:
    - `gradient`: If true, points are colored by local density.
    - `contour`: If true, renders topographic contour lines instead of points.
    - `contour_levels`: Number of levels to render when `contour` is enabled.
- **Labels & Fonts**: `title`, `x_label_size`, `y_label_size`, `x_label_rotation`, `y_label_rotation`, `label_font`, `x_label_font`, `y_label_font`, `title_size`.
- **Options**: `show_scale_labels` (toggles "(linear)" / "(log10)" suffixes).

### `PlotOverlay` (Struct)
Represents a vector path (such as a gate outline) to be drawn on top of a plot.
- `points`: A sequence of vertices in data coordinates.
- `stroke`: CSS color string for the line.
- `fill`: Optional CSS color string for the area inside the path.
- `closed`: Boolean indicating if the path should close back to the start point.

### `DensityGrid` (Struct)
A regular 2D histogram used as a basis for density analysis.
- `nx`, `ny`: Grid resolution (columns/rows).
- `x0`, `x1`, `y0`, `y1`: The bounding box of the grid in data coordinates.
- `values`: A row-major flat vector storing the count of events in each cell.
- **Key Methods**:
    - `at(i, j) -> f64`: Retrieves the value at grid node `(i, j)` with boundary clamping.
    - `max() -> f64`: Finds the peak density in the grid.

---

## Key Functions

### SVG Rendering

#### `dot_plot_svg(...) -> String`
Renders a 2D scatter plot of two parameters.
- **Inputs**: Data matrix, column indices for X and Y, axis labels, event indices to include, optional range limits, a list of `PlotOverlay`s, and a `PlotStyle`.
- **Process**:
    1.  Resolves data ranges (computed automatically if `None`).
    2.  Transforms data points into plot space using the specified `AxisScale`.
    3.  Optional: Generates a `DensityGrid` for gradient or contour rendering.
    4.  Constructs an SVG string including a background rectangle, axes, labels, tick marks, data points (or contours), and overlays.
- **Visual Modes**:
    - **Standard**: Points are drawn with a solid color.
    - **Gradient**: Points are colored using `density_rgb` based on their local density.
    - **Contour**: Uses `contour_segments` to draw topographic lines of density.

#### `histogram_svg(...) -> String`
Renders a 1D histogram for a single parameter.
- **Inputs**: Data matrix, parameter index, labels, event indices, optional value range, number of bins, and `PlotStyle`.
- **Process**:
    1.  Bins the selected data into the specified number of bins.
    2.  Renders the resulting counts as a series of SVG rectangles.
    3.  Includes axes and X-axis tick labels.

### Density and Color Analysis

#### `density_grid(...) -> DensityGrid`
Creates a `DensityGrid` by iterating over a set of points and incrementing the count of the corresponding grid cell.

#### `point_densities(...) -> Vec<f64>`
Calculates the local density for every input point by performing **bilinear interpolation** on the provided `DensityGrid`.

#### `contour_segments(...) -> Vec<[[f64; 2]; 2]>`
Implements the **Marching Squares** algorithm to find linear segments that approximate a specific density `level` within the `DensityGrid`.

#### `density_rgb(t: f64) -> (u8, u8, u8)`
Maps a normalized density value $t \in [0, 1]$ to a piecewise-linear color ramp:
`Blue` $\to$ `Cyan` $\to$ `Yellow` $\to$ `Red`.

#### `format_tick(v: f64, scale: AxisScale) -> String`
Formats an axis tick value for display. It uses scientific notation (e.g., `7e4`) for very large or small values to keep labels concise and avoid overlap.

---

## Implementation Details & Design Patterns

### Coordinate Transformations
The module separates data coordinates from plot coordinates. `AxisScale` handles the non-linear transformation (logarithmic), while the rendering functions handle the linear mapping from transformed data space to pixel space.

### Performance and Robustness
- **Bilinear Interpolation**: Used in `point_densities` to ensure smooth color gradients even with low-resolution grids.
- **Numerical Stability**: Uses `f64::EPSILON` for zero-checks and `.is_finite()` to filter out `NaN` or `Inf` values from the input data.
- **Row-Major Storage**: `DensityGrid` stores values in a flat `Vec<f64>` for better cache locality.

### Dependencies
- `ndarray`: Used for high-performance handling of the event data matrices.
- `serde`: Enables `AxisScale` and `PlotStyle` to be saved/loaded from configuration files.
