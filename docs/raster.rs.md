# src/raster.rs

## Overview
The `raster` module provides functionality to export SVG (Scalable Vector Graphics) content into raster image formats. Since the project's primary plotting engine emits SVG, this module bridges the gap to support common image formats like PNG, JPEG, and TIFF by rasterizing the vector data and encoding it into pixels.

The implementation relies on two primary ecosystems:
- **`resvg`**: Used for parsing the SVG and rendering the vector geometry and text layout.
- **`image`**: Used for managing pixel buffers and encoding them into specific file formats.

## Data Types

### `ImageFormat` (Enum)
Represents the supported raster image formats for export.

**Variants:**
- `Png`: Portable Network Graphics.
- `Jpeg`: Joint Photographic Experts Group.
- `Tiff`: Tagged Image File Format.

**Implemented Methods:**
- `from_path(path: &Path) -> Option<Self>`:
  Infers the `ImageFormat` from the file extension of the provided path. It supports `.png`, `.jpg`/`.jpeg`, and `.tif`/`.tiff` (case-insensitive).
- `extension(self) -> &'static str`:
  Returns the canonical file extension for the format (e.g., `"png"`, `"jpg"`, `"tiff"`).
- `label(self) -> &'static str`:
  Returns a short, human-readable uppercase name (e.g., `"PNG"`, `"JPEG"`, `"TIFF"`).
- `dialog_filters() -> &'static [(&'static str, &'static [&'static str])]`:
  Returns a slice of pairs containing the label and associated extensions, typically used to populate native operating system "Save As" dialog filters.

## Key Functions

### `rasterize_svg`
Converts an SVG string into a raw RGBA pixel buffer.

**Signature:**
```rust
pub fn rasterize_svg(
    svg: &str,
    width: u32,
    height: u32,
    scale: f32,
) -> Result<image::RgbaImage, String>
```

**Internal Logic:**
1. **SVG Parsing**: Uses `resvg::usvg::Tree::from_str` to parse the SVG input.
2. **Text Handling**: To ensure that labels and text within the SVG are rendered correctly, the function initializes a `fontdb::Database`, loads system fonts, and explicitly converts `<text>` elements into paths via `tree.convert_text(&fontdb)`.
3. **Scaling**: Calculates the output dimensions by multiplying the logical `width` and `height` by the `scale` factor. The scale is clamped to a minimum of `0.05` to prevent zero-sized images.
4. **Rendering**: Uses `resvg::Tree::render` to draw the SVG onto a `tiny_skia::Pixmap`.
5. **Alpha Correction**: `tiny-skia` produces pixels with **premultiplied alpha**, whereas the `image` crate expects **straight alpha**. The function iterates through the pixel data and performs the conversion:
   - For each pixel where alpha is not 0 or 255: $\text{color} = \text{color} \times \frac{255}{\text{alpha}}$.
6. **Buffer Creation**: Wraps the processed raw bytes into an `image::RgbaImage`.

### `save_svg_as_image`
A high-level utility that rasterizes an SVG and writes it directly to a file.

**Signature:**
```rust
pub fn save_svg_as_image(
    svg: &str,
    path: &Path,
    width: u32,
    height: u32,
    scale: f32,
) -> Result<ImageFormat, String>
```

**Internal Logic:**
1. **Format Inference**: Calls `ImageFormat::from_path` to determine the output format based on the filename.
2. **Rasterization**: Calls `rasterize_svg` to generate the `RgbaImage` buffer.
3. **File I/O**: Creates the file at the target `path` and wraps it in a `BufWriter` for efficient writing.
4. **Encoding**:
   - **PNG/TIFF**: Encoded directly as RGBA8.
   - **JPEG**: Since JPEG does not support alpha channels, the `RgbaImage` is converted to RGB8 via `to_rgb8()` before encoding. A fixed quality setting of `92` is used.

## Implementation Details & Design Patterns
- **Error Handling**: The module uses `Result<T, String>` for its public API, converting various internal error types (from `resvg`, `image`, and `std::io`) into descriptive strings.
- **Coordinate Space**: The `scale` parameter allows for supersampling (e.g., `scale = 2.0` for 2x resolution), which is a common technique to improve the quality of rasterized vector graphics.
- **Crate Integration**: The module acts as a glue layer between `resvg` (rendering) and `image` (encoding), handling the critical data transformation between premultiplied and straight alpha.

## Dependencies
- `resvg`: Core SVG rendering engine.
- `fontdb`: Used to resolve system fonts for text-to-path conversion.
- `image`: Provides the `RgbaImage` type and encoders for PNG, JPEG, and TIFF.
- `tiny-skia`: The underlying rasterizer used by `resvg`.
