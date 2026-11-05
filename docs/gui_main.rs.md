# src/gui_main.rs

## High-Level Purpose
`src/gui_main.rs` serves as the entry point for the `flowers` GUI application. Its primary responsibility is to bootstrap the graphical user interface using the `eframe` framework, initialize system logging, and handle initial file loading from command-line arguments.

## Detailed Breakdown

### Functions

#### `main()`
The `main` function is the application's entry point. It performs the following sequence of operations:

1. **Logging Initialization**: Calls `env_logger::init()` to enable environment-based logging.
2. **Argument Parsing**: Collects all command-line arguments (skipping the executable name) and converts them into a vector of `PathBuf` objects. These paths represent files the user wishes to open immediately upon startup.
3. **Window Configuration**: Defines `eframe::NativeOptions` to set the initial window properties:
    - **Initial Size**: 1400x900 pixels.
    - **Minimum Size**: 900x600 pixels.
    - **Window Title**: "flowers — flow cytometry analysis".
4. **Application Launch**: Executes `eframe::run_native`, which starts the native GUI event loop.
5. **App Initialization**: Inside the `run_native` closure:
    - Initializes a default instance of `FlowersApp`.
    - Calls `app.open_paths(&paths)` to load any files provided via command-line arguments.
    - Returns the application instance wrapped in a `Box`.

## Implementation Details and Design Patterns

- **Framework**: The module utilizes `eframe` (the framework for `egui`), which follows an immediate-mode GUI pattern.
- **Bootstrap Pattern**: This file acts as a thin wrapper that separates the platform-specific window initialization from the core application logic contained within `FlowersApp`.
- **Error Handling**: The `main` function returns an `eframe::Result`, allowing the framework to handle potential startup failures gracefully.

## Relationships and Dependencies

- **`flowers::gui::FlowersApp`**: This is the core dependency. `gui_main.rs` instantiates `FlowersApp` and hands over control of the application state and rendering to it.
- **`eframe` / `egui`**: Provides the underlying windowing and UI rendering capabilities.
- **`std::env`**: Used to interface with the operating system for command-line arguments.
- **`env_logger`**: Used for diagnostic output.
