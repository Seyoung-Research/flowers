# `src/gui.rs` Documentation

## Overview
`src/gui.rs` implements the graphical user interface for the `flowers` application using the `egui` (via `eframe`) framework. Its primary purpose is to provide an interactive desktop environment for analyzing flow cytometry data (FCS files), specifically focused on gating populations and arranging plots in a layout for export.

## Key Responsibilities
- **Data Visualization**: Rendering high-density scatter plots with support for linear/logarithmic scales, density gradients, and contour views.
- **Gating Toolkit**: Providing tools to draw and manipulate various gate shapes (rectangles, ellipses, polygons, quadrants, and bilinear lines).
- **Population Management**: Managing a hierarchical tree of gated populations and calculating the corresponding event counts and percentages.
- **Layout Editing**: A free-form canvas where users can arrange, resize, and style plot panels for final figure export.
- **State & Undo/Redo**: Tracking the application state and providing a snapshot-based undo/redo mechanism.
- **Export**: Exporting the final layouts as images (SVG, PNG, JPEG, TIFF) and exporting population statistics as CSV.

## Detailed Component Analysis

### Data Models

#### `FlowersApp` (Main Application State)
The central state container for the application. It manages:
- `samples`: List of loaded FCS data.
- `current_sample`: The currently active sample being visualized.
- `gates`: Shared gate definitions applied across samples.
- `pop_indices`: Cached indices of events belonging to each gate for the current sample.
- `children`: Cached hierarchy map of gate children.
- `plot_points`: Pre-calculated data points for the currently active plot to optimize rendering.
- `layout_panels`: Specifications for the figure layout editor.
- `history`: A stack of `UndoState` snapshots for the undo/redo system.
- `theme`: Current visual theme (Dark/Light).

#### `GateDef` and `GateKind`
- **`GateDef`**: Defines a gate's identity, including its name, the X/Y parameters it operates on, its parent in the hierarchy, and its ownership (whether it's specific to one sample or global).
- **`GateKind`**: An enum representing the geometry of the gate:
    - `Rect`: A rotatable rectangle.
    - `Ellipse`: A rotatable ellipse.
    - `Polygon`: A free-form shape defined by vertices.
    - `Quadrant`: One of four quadrants defined by a central crosshair.
    - `Bilinear`: A region bounded by a line segment (above or below).

#### `Interact` and `ManipSession`
Handle the complex state of interacting with gates on a plot:
- `Interact`: Tracks which gate is selected and whether a manipulation (resize, move, rotate) is in progress.
- `ManipSession`: Snapshots the gate's initial state at the start of a drag to ensure delta-based updates are stable.

### Core Logic & Algorithms

#### Population Calculation (`compute_gate_populations`)
Computes the hierarchical gating results. For each gate, it evaluates the gating criteria against the parent population's events. This ensures that a "child" gate only selects events that have already passed through all its ancestors in the gate tree.

#### Plot Rendering (`show_plot`)
Handles the drawing of scatter plots. Key features include:
- **Density Gradients**: Recouring points based on local overlap to visualize high-density regions.
- **Contour Views**: Using a topographic approach to represent data density.
- **Coordinate Mapping**: Converting data-space coordinates to screen-space pixels based on the current axis scales and ranges.

#### Interaction Drive (`drive_interact`)
Processes raw input events (clicks, drags) to determine if the user is creating a new gate, moving an existing one, or adjusting its handles. It uses a tolerance (`tol`) to snap to gate handles.

### Design System
The module implements a custom design system inspired by `flow_r`, using a `Palette` struct to manage colors (Teal primary, slate neutrals). It provides several helper functions for consistent UI components:
- `card_frame`: Rounded, bordered containers.
- `card_head`: Consistent headers with titles and trailing actions.
- `primary_button` / `secondary_button`: Styled action buttons.

## Relationships & Dependencies
- **`crate::project`**: Imports types like `Project`, `ProjectGate`, and `ViewSettings` to facilitate the serialization of the GUI state into a project file.
- **`eframe` / `egui`**: The underlying framework for all UI rendering and event handling.
- **`ndarray`**: Used for efficient processing of the FCS data matrices during population calculations.

## Implementation Details
- **Undo/Redo**: Implemented via `UndoState`, which captures a snapshot of all editable state except the raw FCS data.
- **Performance**: Uses cached `plot_points` and `pop_indices` to avoid re-computing expensive operations (like iterating over millions of events) on every frame.
- **Serialization**: Uses a series of `_id` and `_from_id` helpers to convert internal GUI enums to strings for project storage.
