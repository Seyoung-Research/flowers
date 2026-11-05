# src/statistics.rs

## Overview

The `statistics.rs` module provides the infrastructure for computing and representing statistical summaries of specific "populations" (subsets of events) within a dataset. It allows for the calculation of multiple standard summary statistics across various parameters for a given population, facilitating data analysis and profiling.

## Data Structures

### `Statistic` (Enum)
Represents the type of summary statistic being computed.

| Variant | Description |
| :--- | :--- |
| `Count` | The number of events in the population. |
| `PercentOfParent` | The percentage of the population relative to its parent population. |
| `PercentOfTotal` | The percentage of the population relative to the total dataset size. |
| `Mean` | Arithmetic mean of the parameter values. |
| `Median` | The middle value of the sorted parameter values. |
| `StdDev` | Standard deviation of the parameter values. |
| `Cv` | Coefficient of Variation ($\sigma / \mu$). |
| `GeoMean` | Geometric mean of the parameter values (calculated for positive values only). |
| `Min` | The minimum value observed in the population. |
| `Max` | The maximum value observed in the population. |

### `PopulationStat` (Struct)
A container for a single computed statistic associated with a specific parameter.

- **Fields**:
    - `parameter`: `String` — The name of the parameter (e.g., "Temperature", "Pressure").
    - `statistic`: `Statistic` — The type of statistic being reported.
    - `value`: `f64` — The computed numerical value of the statistic.

### `PopulationProfile` (Struct)
A comprehensive statistical profile of a population, encapsulating overall counts and a collection of specific statistics.

- **Fields**:
    - `name`: `String` — The identifying name of the population.
    - `count`: `usize` — The total number of events in this population.
    - `parent_count`: `Option<usize>` — The size of the parent population, if applicable.
    - `total_count`: `Option<usize>` — The total number of events in the entire dataset, if applicable.
    - `stats`: `Vec<PopulationStat>` — A list of calculated statistics for various parameters.

#### Methods
- `percent_of_parent() -> Option<f64>`: Calculates and returns the percentage of this population relative to the `parent_count`. Returns `None` if `parent_count` is not provided.
- `percent_of_total() -> Option<f64>`: Calculates and returns the percentage of this population relative to the `total_count`. Returns `None` if `total_count` is not provided.

---

## Key Functions

### `compute_profile`
Calculates a full statistical profile for a subset of events.

```rust
pub fn compute_profile(
    name: &str,
    data: &ndarray::Array2<f32>,
    indices: &[usize],
    parameter_names: &[String],
    parent_count: Option<usize>,
    total_count: Option<usize>,
) -> PopulationProfile
```

**Arguments**:
- `name`: The name to assign to the resulting profile.
- `data`: A 2D array where rows represent events and columns represent parameters.
- `indices`: A slice of row indices in `data` that constitute the population.
- `parameter_names`: The labels for the columns in `data`.
- `parent_count`: Optional count of the parent population for percentage calculations.
- `total_count`: Optional total event count for percentage calculations.

**Internal Logic**:
1. **Population Sizing**: Determines the number of events in the population based on the length of `indices`.
2. **Global Percentages**: If `parent_count` or `total_count` are provided, it computes the population percentage and adds them to the `stats` list as pseudo-parameters named `%Parent` and `%Total`.
3. **Parameter Iteration**: For each column in the data matrix:
    - Extracts values corresponding to the provided `indices`.
    - Sorts the values to enable efficient calculation of the median, minimum, and maximum.
    - Computes the **Mean** and **Standard Deviation**.
    - Computes the **Coefficient of Variation** ($\text{StdDev} / \text{Mean}$).
    - Computes the **Median** (averaging the two middle elements for even-sized populations).
    - Computes the **Geometric Mean** by averaging the logarithms of the values and taking the exponent (only if all values are strictly positive; otherwise results in `NaN`).
    - Identifies the **Min** and **Max** values.
    - Stores each result as a `PopulationStat`.

**Returns**: A `PopulationProfile` containing all the computed statistics.

### `print_profile`
A utility function to output a human-readable summary of a `PopulationProfile` to the console.

```rust
pub fn print_profile(profile: &PopulationProfile)
```

It prints the population name, the event count, percentages (if available), and a list of all computed parameter statistics, handling `NaN` values gracefully.

---

## Implementation Details

### Design Patterns
- **Data-Driven**: The module relies heavily on `ndarray` for efficient numerical operations and `serde` for persistence, allowing statistical profiles to be serialized to formats like JSON.
- **Complexity**: The time complexity of `compute_profile` is $O(P \cdot N \log N)$, where $P$ is the number of parameters and $N$ is the number of events in the population, primarily due to the sorting required for median and range calculations.

### Dependencies
- `serde`: Used for `Serialize` and `Deserialize` traits on all profile-related structures.
- `ndarray`: Used for the input data matrix (`Array2<f32>`).
