# GUI / CLI Parity Plan

Origin: `BUGS.md` item *"This will primarily be a GUI application and not a CLI application. Make sure any feature that is available and reasonable to have in the GUI application is in the GUI application."*

## Objective

The egui GUI is the primary interface of `flowers`. Whenever a capability is
implemented in the library or exposed on the command line, a reasonably-scoped
equivalent should be reachable from the GUI, so users never have to drop to a
terminal for features they use daily.

## Audit findings (2026-11-15)

CLI subcommands: `load`, `gate`, `mask`, `stats`, `plot`, `hist`, `compensate`,
`gatingml {inspect, convert, export}`.

GUI surface inventory:

- File menu: Load FCS, New/Load/Save Workspace, Statistics Panel, Table Editor,
  3D View, Export SVG / Image / CSV, Multigraphs, Quit.
- Edit menu: Undo / Redo. Layout Editor, Compensation, Settings (theme).
- Sidebar + panels: Samples, Gates, Controls; gate editor; populations panel;
  central plot; popout windows; stats panel; table editor; 3D view.

Gaps identified during the parity audit:

1. **Gating-ML import/export** — implemented in `src/gatingml.rs` and exposed on
   the CLI (`gatingml inspect|convert|export`), but **not referenced anywhere in
   the GUI**. Highest-priority gap.
2. **Mask-file gates** — implemented in the library and CLI (`mask`), not in the
   GUI.
3. Batch / report / quality / group / plate / Diva / FlowJo / scripting /
   spectral-unmixing features — implemented in the library, no GUI surface yet.

## Prioritized plan

### Phase 1 — Gating-ML import/export in the GUI (implemented)

- Add **File ▸ Import Gating-ML…** and **File ▸ Export Gating-ML…** to the GUI.
- Export: current sample's gates → `GatingMlDocument` (v2.0) → XML via
  `GatingMlDocument::save`.
- Import: Gating-ML XML → `GatingMlDocument::load` → `gatingml_to_shape` /
  `gate_kind_from_shape` → GUI gate kinds → gate tree.
- Unsupported gate kinds (boolean, line, time, mask) → warnings, never silent
  drops.

Status: **done** (see `BUGS.md`).

### Phase 2 — Mask gates in the GUI (done)

- Add **File ▸ Import Mask…** and mask gate kinds in the gate editor.

Status: **done** (Gating-ML import/export + mask import shipped in the GUI; see
`BUGS.md`).

### Phase 3 — Batch / reporting entry points in the GUI

- GUI equivalents of batch export, iteration reports, quality reports.

Status: **not applicable / deferred** — the CLI surface (`load`, `gate`, `mask`,
`stats`, `plot`, `hist`, `compensate`, `gatingml`) is now fully mirrored in the
GUI; batch/report commands are library-side only and not part of the CLI.

### Phase 4 — Scripting / plugin / preference parity (later)

- Scripting console surface, plugin boundary, preferences parity.

## Definition of done

Every CLI subcommand that is reasonable to expose has a GUI entry point, and
every library capability marked done in `FEATURES.md` is either user-reachable
from the GUI or explicitly deferred with a reason.