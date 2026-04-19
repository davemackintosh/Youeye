# youeye — technical stack

## Crate choices

| Concern | Crate | Notes |
|---|---|---|
| UI chrome | `egui` | wgpu-backed; mature docking/panels |
| Canvas renderer | `vello` | Linebender GPU 2D renderer |
| SVG import (reference) | `usvg` / `resvg` | correctness reference for foreign SVG import |
| Layout engine | `taffy` | flexbox/grid; directly drives auto-layout |
| Text | `parley` + `swash` | Linebender stack; pairs with vello |
| Geometry | `kurbo`, `lyon` | bezier math; tessellation/boolean ops |
| Constraint solver | `kiwi` | Cassowary port; ruler-based + pin-to-edge constraints |
| Windowing | `winit` | cross-platform event loop |
| Native menus (Mac/Windows) | `muda` | Tauri's cross-platform native menu crate |
| Per-OS paths | `directories` | config/cache/data dirs |

## Layout pipeline

1. **`taffy`** runs first for auto-layout frames (flex children).
2. **`kiwi`** runs second for constraints on non-flex elements (pin-to-edge, ruler-based).
3. Inside a flex container, flex always wins — rulers are scaffolding only.

## Why egui + vello

Rejected alternatives:

- **Bevy** — game engine optimized for per-frame simulation, whereas a design tool is event-driven.
- **Dioxus** — great for forms/panels but the canvas becomes an escape hatch where all the work lives anyway.

egui + vello gives a desktop-grade chrome + GPU canvas split without fighting either extreme.

## Workspace crates

```
crates/
├── youeye-doc/     # document model; pure, no UI deps
├── youeye-render/  # vello wrapper; scene building, camera, hit testing
├── youeye-io/      # SVG import/export; project folder I/O
└── youeye-app/     # egui + winit + muda; user-facing binary
```

- Don't leak UI types into `youeye-doc`.
- Before adding an alternative crate, check it beats the chosen one on a concrete axis — arbitrary swaps churn the plan.
