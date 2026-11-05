# Layout Module (`src/layout.rs`)

The `layout` module is responsible for composing multiple pre-rendered SVG plots into a single composite SVG image for export or display. It provides two primary methods of arrangement: a structured grid and a free-form canvas.

## High-Level Purpose

The primary goal of this module is to take several self-contained SVG documents (typically produced by plotting functions like `dot_plot_svg`) and arrange them into a single figure. 

A key technical decision in this module is the **inlining of SVG contents**. Rather than nesting `<svg>` tags—which can lead to rendering issues or "missing plots" in some SVG viewers due to nested viewport conflicts—the module extracts the inner content of each panel and wraps it in an SVG group element (`<g>`). This ensures maximum portability across different SVG viewers and raster export tools.

## Data Structures

### `LayoutPanel`
Represents a single plot component to be included in a layout.
- `title`: A string label displayed above the plot.
- `svg`: The full SVG document string of the rendered plot.

### `LayoutOptions`
Configuration settings that control the visual appearance and arrangement of the composite layout.
- `columns`: The number of columns in the grid layout.
- `cell_width`: The width of an individual plot cell.
- `cell_height`: The height of an individual plot cell.
- `title_height`: Vertical space (in pixels) reserved above each panel for its title.
- `padding`: Global padding around the entire figure and between cells.
- `background`: The CSS color for the canvas background (e.g., `"#ffffff"`).
- `text_color`: The CSS color for the panel titles.
- `font_family`: The font family used for titles (e.g., `"sans-serif"`). This is explicitly set to prevent fallback to mismatched fonts during rasterization.
- `title_size`: The font size of panel titles.

### `LayoutPlacement`
Used for free-form layouts, providing explicit control over where a panel is placed and how it is scaled.
- `title`: The label for the plot.
- `svg`: The full SVG document string.
- `svg_width`: The original (natural) width of the input SVG.
- `svg_height`: The original (natural) height of the input SVG.
- `x`: The X-coordinate of the panel's top-left corner on the canvas.
- `y`: The Y-coordinate of the panel's top-left corner on the canvas.
- `width`: The desired display width on the canvas.
- `height`: The desired display height on the canvas.
- `title_size`: Individual font size for this panel's title.

## Key Functions

### `render_layout`
Arranges a slice of `LayoutPanel`s into a grid based on the provided `LayoutOptions`.

**Arguments:**
- `panels`: A slice of `LayoutPanel` structs to be rendered.
- `opts`: A reference to `LayoutOptions` for grid and styling configuration.

**Logic:**
1. Calculates the required number of rows based on the number of panels and the requested column count.
2. Computes the total canvas dimensions including padding.
3. Generates the root `<svg>` tag and a background rectangle.
4. Iterates through panels, calculating the `(x, y)` position for each cell.
5. Renders the panel title centered above the plot.
6. Extracts the inner content of the panel's SVG using `svg_inner` and wraps it in a `<g>` element with a `translate` transformation.

### `render_layout_free`
Composes panels into a single SVG canvas using explicit coordinates and sizes. This is primarily used by the GUI layout editor.

**Arguments:**
- `canvas_width`: Width of the output canvas.
- `canvas_height`: Height of the output canvas.
- `placements`: A slice of `LayoutPlacement` structs.
- `opts`: A reference to `LayoutOptions` for general styling (background, font family, etc.).

**Logic:**
1. Initializes the SVG canvas and background.
2. For each placement:
    - Renders the title above the specified position.
    - Calculates scale factors (`sx`, `sy`) by dividing the desired `width`/`height` by the natural `svg_width`/`svg_height`.
    - Extracts the inner SVG content and wraps it in a `<g>` element applying both `translate` (for position) and `scale` (to fit the desired size) transformations.

### Internal Helpers
- `svg_inner(svg: &str) -> &str`: Extracts the markup between the opening `<svg ...>` tag and the closing `</svg>` tag.
- `svg_escape(s: &str) -> String`: Escapes XML special characters (`&`, `<`, `>`, `"`) to ensure the resulting SVG is well-formed.

## Implementation Details & Design Patterns

- **Portability**: The use of `svg_inner` to avoid nested `<svg>` tags is a specific optimization for compatibility with various SVG renderers.
- **Coordinate System**: The module uses floating-point calculations for positions and scaling to maintain precision in the final SVG output.
- **Scaling Strategy**: In `render_layout_free`, the scaling is performed via the SVG `scale()` transformation on the group level, allowing the internal coordinates of the original plot to remain unchanged while the rest of the plot is resized.

## Relationships

- **Dependencies**: Consumes SVG strings typically generated by the `plot` module (e.g., `crate::plot::dot_plot_svg`).
- **Integration**: Serves as the backend for the GUI layout editor, where users can drag and resize `LayoutPlacement` objects before exporting the final composite image.
