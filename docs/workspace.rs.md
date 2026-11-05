# Workspace Module (`src/workspace.rs`)

## Overview
The `workspace.rs` module defines the primary data structures used to manage an experiment's state. It serves as the central coordinator for samples, their associated FCS (Flow Cytometry Standard) data, the hierarchical gating structures (populations), and the cached statistical profiles of those populations.

## Data Structures

### `Population`
A `Population` represents a node in a hierarchical tree of gated events.

- **Purpose**: To track which specific events (via indices) from a sample's data matrix pass a particular gate and to maintain a parent-child relationship for nested gating.
- **Fields**:
    - `name`: `String` - The display name of the population.
    - `event_indices`: `Vec<usize>` - Indices of events that belong to this population.
    - `children`: `Vec<Rc<RefCell<Population>>>` - A list of sub-populations derived from this one.
    - `gate_summary`: `Option<String>` - A description of the gate used to isolate this population.

- **Key Methods**:
    - `new(name: &str, indices: Vec<usize>) -> Rc<RefCell<Self>>`: Constructor that wraps the population in `Rc<RefCell<...>>` to allow shared ownership and interior mutability across the hierarchy.
    - `add_child(&mut self, child: Rc<RefCell<Self>>)`: Appends a sub-population to the current node.
    - `count(&self) -> usize`: Returns the total number of events in the population.
    - `to_snapshot(root: &Rc<RefCell<Self>>) -> PopulationSnapshot`: A recursive function that converts the runtime population tree into a serializable snapshot.

### `PopulationSnapshot`
A simplified version of `Population` designed for serialization.

- **Purpose**: Since `Population` uses `Rc<RefCell<...>>` and contains large index vectors, `PopulationSnapshot` provides a way to save the hierarchy and event counts to a project file without persisting the raw event indices or complex pointers.
- **Fields**:
    - `name`: `String`
    - `count`: `usize`
    - `gate_summary`: `Option<String>`
    - `children`: `Vec<PopulationSnapshot>` (recursive)

### `Sample`
Represents a single FCS data file and its associated analysis state.

- **Fields**:
    - `id`: `String` - Unique identifier for the sample.
    - `fcs_file`: `FcsFile` - The parsed data and metadata from the FCS file.
    - `root_populations`: `Vec<Rc<RefCell<Population>>>` - The top-level populations in the sample's gating tree.
    - `gates`: `Vec<GatedPopulation>` - A flat list of gate definitions applied to the sample.

### `Workspace`
The top-level container for an entire experimental session.

- **Purpose**: To manage multiple samples and cache statistical results across them.
- **Fields**:
    - `samples`: `HashMap<String, Sample>` - Map of sample IDs to their respective `Sample` objects.
    - `sample_order`: `Vec<String>` - Maintains a stable order of samples for UI display.
    - `profiles`: `HashMap<String, PopulationProfile>` - A cache of computed statistical profiles, keyed by a combined string of `sample_id::population_path`.

- **Key Methods**:
    - `add_sample(&mut self, id: &str, fcs_file: FcsFile)`: Integrates a new sample into the workspace.
    - `add_root_population(&mut self, sample_id: &str, name: &str, indices: Vec<usize>)`: Initializes a top-level population for a specific sample.
    - `compute_profile_for(&mut self, sample_id: &str, population_path: &str, indices: &[usize]) -> PopulationProfile`: 
        - Extracts parameter names from the `FcsFile`.
        - Calls `compute_profile` from the statistics module.
        - Caches the resulting `PopulationProfile` in the `profiles` map using the key `{sample_id}::{population_path}`.

## Implementation Details & Design Patterns

### Memory Management and Ownership
The module uses `Rc<RefCell<Population>>` for the population hierarchy. This design is chosen because:
1. **Shared Ownership**: Sub-populations may need to be referenced by both the parent node and external management lists.
2. **Interior Mutability**: Gating operations often require updating the indices or properties of a population node while it is still held within a tree structure.

### Serialization Strategy
`Workspace` and `Sample` are not directly serializable because they contain non-serializable types (`FcsFile` and `Rc<RefCell<...>>`). The project employs a **Snapshot Pattern**:
- Runtime data is converted into `PopulationSnapshot` objects for persistence.
- FCS metadata and file paths are handled separately from the population hierarchy during save/load operations.

### Caching Mechanism
To optimize performance, `Workspace` implements a profiling cache. Computing statistical summaries (means, medians, etc.) for large event sets can be expensive; by storing these in the `profiles` map, the application can retrieve results instantly when the user switches views or re-opens a population.

## Relationships & Dependencies
- **`crate::fcs::FcsFile`**: Provides the raw data matrix and metadata (parameter names, event counts) used by `Sample` and `Workspace`.
- **`crate::gating::GatedPopulation`**: Used by `Sample` to store the metadata describing how a population was gated.
- **`crate::statistics`**: The `compute_profile` function is the engine used by `Workspace::compute_profile_for` to generate the `PopulationProfile` objects.
