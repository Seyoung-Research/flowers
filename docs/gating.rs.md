# gating.rs

The `gating.rs` module provides the core abstractions and implementations for "gating" in flow cytometry. Gating is the process of isolating specific populations of cells or events from a larger sample based on their expression of one or more parameters (channels).

## High-Level Purpose
The primary responsibility of this module is to define how events are selected from a dataset. It provides a variety of 2D geometric shapes (gates) and a mechanism for combining these shapes using boolean logic to create complex, hierarchical gating strategies.

## The `Gate` Trait

The `Gate` trait is the fundamental abstraction for any selection mechanism.

### Methods
- `contains(&self, event: ArrayView1<f32>) -> bool`: The core point-in-gate test. It determines if a single event (represented as a view into a parameter array) falls within the gate's boundaries.
- `filter(&self, data: &ndarray::Array2<f32>) -> Vec<usize>`: A default implementation that iterates through all events in a 2D array and returns the indices of those that satisfy the `contains` test.
- `parameter_indices(&self) -> Vec<usize>`: Returns the indices of the parameters (channels) that this gate depends on. For most 2D gates, this returns the indices for the X and Y axes.

---

## Concrete Gate Implementations

### `RectangularGate`
Defines an axis-aligned or rotated rectangular region.
- **Fields**: `param_idx_x`, `param_idx_y`, `min_x`, `max_x`, `min_y`, `max_y`, and an optional `angle` (radians) for rotation about the center.
- **Logic**: If the angle is 0, it performs a simple range check. If rotated, it transforms the event point into the rectangle's local coordinate frame before performing the range check.

### `EllipseGate`
Defines a rotated elliptical region.
- **Fields**: `param_idx_x`, `param_idx_y`, `center_x`, `center_y`, `radius_x`, `radius_y`, and an optional `angle` (radians).
- **Logic**: Transforms the point into the ellipse's local frame and checks if the normalized distance from the center is $\le 1.0$ (using the ellipse equation $\frac{x^2}{a^2} + \frac{y^2}{b^2} \le 1$).

### `PolygonGate`
Defines an arbitrary closed polygon.
- **Fields**: `param_idx_x`, `param_idx_y`, and a list of `vertices`.
- **Logic**: Implements the **even-odd rule** (Ray Casting algorithm) to determine if a point is inside the polygon.

### `QuadrantGate`
Splits a 2D plane into four quadrants using a vertical and horizontal line meeting at $(x, y)$.
- **Fields**: `param_idx_x`, `param_idx_y`, `x`, `y`, and a `Quadrant` enum (`UpperLeft`, `UpperRight`, `LowerLeft`, `LowerRight`).
- **Logic**: Simple comparison of the event's coordinates against the split point.

### `BilinearGate`
Splits a 2D plane into four wedge-shaped regions using two intersecting lines.
- **Fields**: `param_idx_x`, `param_idx_y`, `center_x`, `center_y`, `angle1`, `angle2`, `radius` (for rendering), `side1`, and `side2`.
- **Logic**: Uses the signed distance of a point from each line. A point is inside if it lies on the specified side (positive or negative) of both lines.

### `LineGate`
Selects events on one side of a line segment, restricted to the segment's horizontal extent.
- **Fields**: `param_idx_x`, `param_idx_y`, `x1`, `y1`, `x2`, `y2`, and a `below` boolean.
- **Logic**:
    1. Checks if the event's X coordinate is within the horizontal bounds $[x_{min}, x_{max}]$.
    2. Calculates the Y value of the line at the event's X coordinate.
    3. Checks if the event's Y is above or below that line value based on the `below` flag.

---

## Boolean Composition

### `BooleanGate` and `BooleanOp`
To allow for complex gating (e.g., "Events that are in Gate A AND (Gate B OR NOT Gate C)"), the module implements a composite pattern via `BooleanGate`.

`BooleanGate` is an enum with three variants:
- `Leaf(Box<dyn Gate>)`: Wraps a concrete `Gate` implementation.
- `Binary { op: BooleanOp, left: Box<BooleanGate>, right: Box<BooleanGate> }`: Combines two gates using `And` or `Or`.
- `Unary { op: BooleanOp, child: Box<BooleanGate> }`: Negates a gate using `Not`.

This structure allows the construction of an arbitrary tree of gates, where the `contains` method recursively evaluates the boolean logic of the tree.

---

## Gated Population Result

### `GatedPopulation`
A utility structure used to report the results of a gating operation.
- **Fields**:
    - `name`: The name given to this population (e.g., "Lymphocytes").
    - `event_indices`: The list of indices of events that passed the gate.
    - `gate_summary`: A string description of the gate used to create the population.
- **Key Method**: `count()` returns the number of events in the population.

## Design Patterns and Implementation Details
- **Trait Polymorphism**: The use of `dyn Gate` in `BooleanGate` allows the system to treat different geometric shapes uniformly.
- **Local Frame Transformation**: For rotated shapes (Rectangle, Ellipse), the implementation rotates the input point by $-\theta$ around the center. This allows the `contains` check to be performed as if the shape were axis-aligned, significantly simplifying the logic.
- **Serialization**: Most gate types derive `Serialize` and `Deserialize`, allowing gating configurations to be saved and loaded from files.
- **Efficiency**: The `filter` method leverages `ndarray` for efficient iteration over event data.
