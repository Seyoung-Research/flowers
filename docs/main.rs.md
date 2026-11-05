# src/main.rs

## High-Level Purpose
`src/main.rs` implements the Command Line Interface (CLI) for the `flowers` tool. It acts as the primary entry point for users to interact with Flow Cytometry Standard (FCS) files, providing tools for data inspection, population gating, statistical analysis, visualization, and compensation correction.

## CLI Structure

### Structs
#### `Cli`
The root configuration for the CLI, leveraging the `clap` crate for argument parsing.
- **Fields**:
    - `command`: A `Commands` enum representing the specific action the user wants to perform.

### Enums
#### `Commands`
Defines the set of available operations supported by the `flowers` utility.

| Command | Purpose | Key Arguments |
| :--- | :--- | :--- |
| `Load` | Parses an FCS file and prints a summary of its metadata and data matrix. | `fcs` (Path) |
| `Gate` | Applies a specific geometric gate to an FCS file and reports the count/percentage of events that pass. | `fcs`, `population`, `gate`, `x`, `y`, `min_x`, `max_x`, `min_y`, `max_y` |
| `Stats` | Gating a population using a rectangular gate and computing descriptive statistics (mean, median, SD, CV, etc.). | `fcs`, `population`, `x`, `y`, `min_x`, `max_x`, `min_y`, `max_y` |
| `Plot` | Renders a 2D dot plot of two specified parameters as an SVG file. | `fcs`, `out`, `x`, `y` |
| `Hist` | Renders a 1D histogram of a single parameter as an SVG file. | `fcs`, `out`, `p` |
| `Compensate` | Applies a spillover correction matrix (from a CSV) to the FCS data. | `fcs`, `matrix` |

#### `GateType`
Specifies the geometric shape of the gate to be used in the `Gate` command.
- `Rect`: Rectangular bounds.
- `Ellipse`: Elliptical bounds.
- `Quadrant`: Split-screen quadrant gating (defaults to Upper Right).
- `Bilinear`: Angular sector gating.

## Key Functions

### `main()`
The entry point of the application.
- Initializes `env_logger`.
- Parses CLI arguments into the `Cli` struct.
- Dispatches the requested command to the `run` function.
- Handles top-level errors by printing them to `stderr` and exiting with code 1.

### `run(command: Commands) -> Result<(), anyhow::Error>`
The core logic dispatcher that executes the requested operation.

#### Implementation Highlights:
- **Gating Logic**: In the `Gate` command, it dynamically constructs a `Box<dyn Gate>` based on the `GateType`.
    - For `Ellipse`, it derives the center and radii from the provided min/max bounds.
    - For `Bilinear`, it defines a sector from $45^\circ$ to $315^\circ$ (using $\pi/4$ and $3\pi/4$ as markers) and calculates a radius based on the largest bound.
- **Statistics**: In the `Stats` command, it uses a `RectangularGate` to isolate events and then calls `compute_profile` and `print_profile` from the `statistics` module.
- **Visualization**: Leverages the `plot` module's `dot_plot_svg` and `histogram_svg` functions to generate SVG output, which is then written to disk via `fs::write`.
- **Compensation**: Reads a CSV matrix via `parse_csv_matrix`, initializes a `CompensationEngine`, and applies it to the data matrix.

### Helper Functions
- `load_fcs(path: &PathBuf)`: Reads the raw bytes of a file and parses them into an `FcsFile` using `FcsParser`.
- `param_name(fcs: &FcsFile, idx: usize)`: Safely retrieves the name of a parameter by index, providing a fallback (e.g., "P0") if the index is out of bounds.
- `parse_csv_matrix(text: &str)`: Converts a comma-separated string into an `ndarray::Array2<f32>`. It includes validation to ensure the matrix is not empty and is not "ragged" (all rows must have the same number of columns).
- `array2_from_flat(flat: &[f32], nrows: usize, ncols: usize)`: A utility to build an `ndarray::Array2` from a flat slice, validating that the slice length matches the expected dimensions.

## Design Patterns and Implementation Details
- **Trait Objects**: The use of `Box<dyn Gate>` allows the `Gate` command to treat different gating geometries polymorphically.
- **Error Handling**: Extensive use of `anyhow::Error` for flexible error propagation across the CLI.
- **Data Handling**: Uses `ndarray` for efficient manipulation of the large event-by-parameter matrices typical in flow cytometry.

## Module Relationships
`src/main.rs` serves as the orchestrator, depending on the following internal modules:
- `flowers::fcs`: For parsing the binary FCS file format.
- `flowers::gating`: For implementing the `Gate` trait and specific gate shapes (`RectangularGate`, `EllipseGate`, etc.).
- `flowers::statistics`: For computing population profiles.
- `flowers::plot`: For rendering SVG visualizations.
- `flowers::compensation`: For applying linear spillover correction matrices.
