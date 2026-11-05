# gen_fcs.rs

## Overview
`gen_fcs.rs` is a standalone example utility designed to generate a realistic, synthetic FCS 3.0 (Flow Cytometry Standard) file. Its primary purpose is to provide a consistent, deterministic dataset for end-to-end CLI testing of the `flowers` library and its associated tools.

The generated file (`sample.fcs`) contains:
- **4,000 events**
- **4 parameters**: FSC-H, SSC-H, FL1-H, and FL2-H
- **Two distinct clusters**: To simulate biological populations, the data is generated such that some events are low on all channels while others are high on the fluorescence channels (FL1/FL2).

## Detailed Breakdown

### Functions

#### `ptr(n: usize) -> [u8; 8]`
A helper function used to generate the fixed-width 8-byte pointers required by the FCS 3.0 header specification.
- **Arguments**: `n` (the byte offset).
- **Returns**: An array of 8 bytes containing the right-aligned ASCII representation of the integer `n`.

#### `main()`
The main execution loop that constructs the FCS file in memory and writes it to disk.

**Process Flow:**
1. **Metadata Construction**:
   - Defines total event count (`$TOT`) and parameter count (`$PAR`).
   - Defines parameter names (e.g., "FSC-H") and descriptions (e.g., "Forward Scatter-Height").
   - Sets the byte order to little-endian (`$BYTEORD\1,2,3,4\`) and mode to linear (`$MODE\L\`).
2. **Header Assembly**:
   - Writes the magic string `FCS3.0`.
   - Calculates and writes pointers to the text, data, and analysis segments using the `ptr()` helper.
3. **Data Generation**:
   - Uses a deterministic pseudo-random number generator (LCG) seeded with `42`.
   - Iterates through 4,000 events.
   - For each event, it decides whether it belongs to "Cluster A" (low base values) or "Cluster B" (high base values).
   - Adds random noise to each parameter value to create Gaussian-like clusters.
   - Encodes values as little-endian bytes.
4. **File Output**:
   - Writes the resulting buffer to `sample.fcs` using `std::fs::write`.

## Design Patterns and Implementation Details

### Deterministic Randomness
Instead of using the `rand` crate, the implementation uses a simple manual LCG:
```rust
rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
```
This ensures that every time the example is run, it produces the exact same `sample.fcs` file, which is critical for regression testing.

### FCS 3.0 Compliance
The code manually implements the FCS 3.0 binary format:
- **Header**: Fixed-size pointers.
- **Text Segment**: Key-value pairs delimited by backslashes (`\`).
- **Data Segment**: Contiguous blocks of binary floating-point numbers.

## Relationships and Dependencies
- **External Dependencies**: None (uses only `std`).
- **Internal Role**: Acts as a producer for the `flowers` parser. While it doesn't import other modules from the project, it is designed to create files that satisfy the input requirements of the `flowers` library.
