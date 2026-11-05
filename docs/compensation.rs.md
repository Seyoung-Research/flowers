# Compensation Module (`src/compensation.rs`)

## Overview
The `compensation` module provides the functionality to apply a compensation matrix to multi-channel datasets. This is typically used in signal processing to correct for "spillover" or crosstalk, where a signal from one channel leaks into another. The primary mechanism is a matrix multiplication between the input data and a pre-defined compensation matrix.

## Data Structures

### `CompensationEngine`
A structure that holds the state required to perform compensation.

- **Fields**:
    - `matrix`: `Array2<f32>` — The compensation (spillover) matrix. 
        - **Rows**: Must match the number of original input channels.
        - **Columns**: Represent the number of corrected output channels.

### `CompensationError`
An enum representing errors that can occur during the compensation process.

- **Variants**:
    - `DimensionMismatch { expected: String, found: String }`: Occurs when the number of channels in the input dataset does not match the number of rows in the compensation matrix.

## Functions

### `CompensationEngine::new`
Constructor for the `CompensationEngine`.

- **Arguments**: 
    - `matrix`: `Array2<f32>` — The compensation matrix to be used for all subsequent `compensate` calls.
- **Returns**: `Self` (a new `CompensationEngine` instance).

### `CompensationEngine::compensate`
Applies the compensation matrix to the provided dataset.

- **Arguments**: 
    - `data`: `&Array2<f32>` — A reference to the input data matrix with dimensions `(events x channels)`.
- **Returns**: `Result<Array2<f32>, CompensationError>`
    - **Success**: Returns a new `Array2<f32>` containing the compensated data.
    - **Failure**: Returns `CompensationError::DimensionMismatch` if the input channel count is incompatible with the engine's matrix.
- **Logic**:
    1. Extracts the number of channels from the input `data` dimensions.
    2. Extracts the number of rows from the `CompensationEngine`'s `matrix`.
    3. Validates that `channels == mat_rows`.
    4. Performs a matrix dot product: `[Events x Channels] * [Channels x CorrectedChannels]`.
    5. Returns the resulting matrix.

## Implementation Details
- **Linear Algebra**: The module leverages the `ndarray` crate for efficient matrix operations. The `dot` method is used to perform the matrix multiplication.
- **Error Handling**: Uses the `thiserror` crate to provide descriptive, typed error messages for dimension mismatches.

## Relationships and Dependencies
- **Dependencies**: 
    - `ndarray`: Used for `Array2` and matrix multiplication.
    - `thiserror`: Used for implementing the `Error` trait on `CompensationError`.
- **Role in Project**: This module acts as a processing step in the data pipeline, transforming raw channel-based data into corrected data based on a calibration matrix.
