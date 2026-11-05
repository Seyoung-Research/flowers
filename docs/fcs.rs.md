# FCS Module (`src/fcs.rs`)

The `fcs.rs` module provides a robust parser for Flow Cytometry Society (FCS) files, supporting versions 2.0, 3.0, and 3.1. FCS files are the standard format for storing high-dimensional data collected by flow cytometers, consisting of a fixed-size header, a metadata text section, and a binary data segment containing measurement values for each event (cell/particle).

## High-Level Purpose

The primary responsibility of this module is to transform a raw binary FCS stream into a structured `FcsFile` object. This involves:
1. Validating the file version and reading segment pointers from the header.
2. Parsing the text-based keyword section to extract metadata and configuration.
3. Interpreting binary data according to specified byte orders and data types.
4. Loading the event data into a high-performance matrix for analysis.

---

## Data Structures

### `FcsFile`
The central data structure representing a fully parsed FCS file.

| Field | Type | Description |
| :--- | :--- | :--- |
| `version` | `String` | The FCS version (e.g., "FCS3.1"). |
| `keywords` | `HashMap<String, String>` | A collection of all metadata keywords extracted from the TEXT section. |
| `parameters` | `Vec<Parameter>` | A list of defined measurement channels. |
| `event_count` | `usize` | Total number of events recorded in the file. |
| `data` | `Array2<f32>` | A 2D matrix of shape `(events, parameters)` containing the measurement values. |

**Note:** `FcsFile` does not implement `Serialize` because `ndarray::Array2` lacks a default serde implementation. Data serialization is intended to be handled via higher-level snapshots.

### `Parameter`
Defines a single measurement channel (parameter) within the FCS file.

| Field | Type | Description |
| :--- | :--- | :--- |
| `name` | `String` | The identifier for the channel (e.g., "FSC-H"). |
| `description` | `String` | A human-readable description (e.g., "Forward Scatter-Height"). |
| `range` | `f32` | The measurement range of the channel. |
| `exponent` | `f32` | The exponent used for scaling (0 for linear). |
| `min` | `f32` | Computed display minimum. |
| `max` | `f32` | Computed display maximum. |

### `FcsError`
An enum representing all possible failure states during parsing, powered by `thiserror`.

- `Io`: Standard I/O failures.
- `Utf8`: Failures when converting bytes to UTF-8 strings.
- `ParseInt`: Failures when parsing integers from keywords.
- `Parse`: General FCS parsing errors (e.g., invalid header pointers).
- `InvalidVersion`: The file version is not supported.
- `UnsupportedByteOrder`: The specified byte order is not recognized.
- `UnsupportedDataType`: The specified data type (e.g., not 'F' or 'D') is not supported.
- `MissingKeyword`: A required keyword (like `$TOT` or `$PAR`) is missing.

---

## Implementation Details

### `FcsParser`
The `FcsParser` is a stateless utility providing the entry points for parsing.

#### Key Methods
- **`parse<R: Read + Seek>(mut reader: R) -> Result<FcsFile, FcsError>`**
  The core parsing pipeline:
  1. **Header Analysis**: Reads the 58-byte fixed header to locate the TEXT and DATA segments.
  2. **Text Parsing**: Seeks to the TEXT segment and parses delimiter-separated keywords.
  3. **Configuration Extraction**: Retrieves critical values such as total events (`$TOT`), parameter count (`$PAR`), byte order (`$BYTEORD`), and data type (`$DATATYPE`).
  4. **Parameter Definition**: Iteratively builds `Parameter` structs using the `$PnN`, `$PnS`, `$PnE`, and `$PnR` keywords.
  5. **Data Extraction**: Seeks to the DATA segment and reads the binary values. It supports:
     - 32-bit floats (`F`)
     - 64-bit doubles (`D`), which are cast to `f32` for consistency.
     - Both Little-Endian and Big-Endian byte orders.
- **`parse_bytes(bytes: &[u8]) -> Result<FcsFile, FcsError>`**
  A convenience method that wraps a byte slice in a `Cursor` and calls `parse`.

### Internal Helpers
- **`read_header_pointer`**: Parses the 8-byte, space-padded, right-justified offsets in the header.
- **`parse_text_section`**: A specialized parser for the FCS TEXT segment that handles delimiters and escaped delimiters (doubled delimiters).
- **`keyword_str`, `keyword_usize`, `keyword_f32`**: Type-safe wrappers for extracting and parsing values from the keywords map.

---

## Design Patterns and Dependencies

- **Generic I/O**: By using `R: Read + Seek`, the parser is decoupled from the specific storage medium, allowing it to read from files on disk or buffers in memory.
- **Matrix Storage**: Utilizes `ndarray::Array2` to store event data, ensuring that subsequent analytical operations (like gating or scaling) are computationally efficient.
- **Error Propagation**: Uses the `?` operator extensively with a custom `FcsError` type to provide precise feedback on why a file failed to parse.
- **External Dependencies**:
    - `ndarray`: For the 2D data array.
    - `byteorder`: For endian-aware reading of binary floats.
    - `thiserror`: For ergonomic error definitions.
    - `serde`: For serializing the `Parameter` metadata.

## Relationships
The `fcs` module acts as the primary ingestion layer for raw flow cytometry data. The resulting `FcsFile` is typically passed to other components of the system to create population snapshots, perform data normalization, or execute gating algorithms.
