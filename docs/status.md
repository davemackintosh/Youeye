# youeye — status & next steps

Running log of where the project is and what's next. Update after each working session so any machine (or a fresh Claude session) can pick up cold.

Last updated: **2026-04-19**.

## Current phase

**Phase 1 — Foundation** ✅ complete.

- 1a — Workspace scaffold, egui + winit + wgpu window with chrome (toolbar, layers, inspector, status bar, menu bar).
- 1b — Vello canvas with pan/zoom, rendered into an offscreen `Rgba8Unorm` texture registered as an egui native texture.

## How to run

```bash
cargo run -p youeye-app
```

Linux needs the usual wgpu/winit system deps — see `.github/workflows/ci.yml` for the apt list (Arch-ish equivalents: `wayland libxkbcommon vulkan-headers vulkan-icd-loader mesa`).

## Canvas controls

- **Pan** — Space+drag, or middle-mouse drag.
- **Zoom** — ⌘/Ctrl + scroll wheel, anchored on the pointer.

## Stack version pins (important)

Pinned in the workspace `Cargo.toml`. The current alignment point is **wgpu 27**:

| Crate | Version | Reason |
|---|---|---|
| `egui`, `egui-winit`, `egui-wgpu` | `0.33` | Uses wgpu 27 |
| `wgpu` | `27` | Shared across egui-wgpu and vello |
| `vello` | `0.7` | Latest version that still uses wgpu 27 |
| `kurbo` | `0.13` | What vello 0.7 → peniko 0.5 pulls |
| `winit` | `0.30` | Stable ApplicationHandler API |
| `muda` | `0.17` | Native menus on Mac/Windows only (target-cfg'd off Linux) |

**Bumping these is not trivial.** `egui-wgpu 0.34` requires wgpu 29, but `vello 0.8` requires wgpu 28 — no single wgpu version satisfies both, so you can't just upgrade one. Wait for either vello 0.9 (likely on wgpu 29) or egui-wgpu 0.35 (likely on wgpu 30); if they ever align, bump both together.

## Task list

Tasks are tracked in-session with the task tool — not durable across machines. This list is the durable version.

### Done ✅

- Copy design docs from memory into `docs/`.
- Set up workspace Cargo.toml and 4 crates (`youeye-doc`, `youeye-render`, `youeye-io`, `youeye-app`).
- Implement youeye-app window with egui + winit + wgpu.
- Implement menu abstraction (muda native on Mac/Win, egui in-window on Linux).
- Cross-platform hygiene: modifier-key abstraction, `directories` wrapper, `\n` line-ending constant.
- CI matrix for Linux, Mac, Windows.
- Implement vello canvas with pan/zoom.

### Phase 2 — Document model + SVG round-trip

1. **`youeye-doc`: node types mirroring SVG.** Group, Rect, Ellipse, Path, Text, Frame. Each node carries an id, a `kurbo::Affine` transform, fill/stroke, and a `youeye:` attribute bag for non-SVG extensions (`layout`, `override-*`, `type="ruler"`, etc).
2. **`youeye-doc`: tokens + variables.** Parse the SVG `<style>` root for `--token-*` / `--var-*` custom properties. Treat them as a first-class dictionary in the model so the inspector can list / enforce them.
3. **`youeye-io`: SVG parser.** Two paths:
   - *Own SVG:* preserve every `youeye:*` attribute and all CSS custom properties verbatim. Round-trip must be byte-exact on a no-op load+save.
   - *Foreign SVG:* best-effort via `usvg` as reference; flatten to raw shapes.
4. **`youeye-io`: SVG serializer.** `\n` line endings. Sorted attributes for deterministic diffs. Pretty-printed.
5. **`youeye-app`: project I/O.** Folder-as-project (`project.yeye.json` + `screens/*.svg` + `components.svg` + `assets/`). Hook into File > New / Open / Save menu actions.
6. **`youeye-app`: layers panel.** Bind to the current screen's node tree; selection, rename, reorder, visibility toggle, lock.
7. **`youeye-render`: scene builder.** Walk the doc tree, emit a `vello::Scene`. Replace the hardcoded test content in `canvas.rs::build_scene`.

### Phase 3 — Auto-layout (taffy)

- Integrate `taffy` for frame nodes. Flex direction / gap / padding / justify / align UI.
- Store as `youeye:layout="flex"` + related namespaced attrs.
- Inspector UI: auto-layout controls for Frame nodes, with gap/padding pickers that default to rhythm multiples.

### Phase 4 — Tokens + variables UI with enforcement

- Tokens panel (right sidebar, below Inspector). Add/edit/reorder/delete tokens. Palette grouping via `project.yeye.json`.
- Variables panel, similarly. Expression editor with autocomplete for token names.
- All colour/size/font pickers gain a "token-first" mode. Inspector shows an "off-token" chip on raw values.
- Mode switcher in the top bar; CSS media queries + class modifiers drive rendering.

### Phase 5 — Rulers + constraints (kiwi)

- Ruler as a node type, rendered as `<line youeye:type="ruler" style="display:none">` in SVG.
- Hierarchical: rulers can live at any level, scoped to their parent frame.
- Integrate `kiwi` (Cassowary port). Layout pipeline: taffy first, then kiwi on non-flex elements.
- UI to create rulers (new tool? keyboard shortcut?) and to define constraints between elements and rulers.

### Phase 6 — Primitive tools

- Rect, ellipse, line, polygon, text, pen.
- Boolean ops on paths (`kurbo` / `lyon`).
- Transform handles on the canvas (move / resize / rotate).

### Phase 7 — Components

- Reusable components backed by `<symbol>` + `<use>`.
- Instance overrides via `youeye:override-*` attrs (text, colour, variant).
- Variant picker UI.

### Phase 8 — Polish + distribution

- Export: SVG (already the native format), PNG via vello headless render, PDF later.
- Fonts: parley integration with system + user-library fonts.
- Packaging: `cargo-packager` for `.app` (signed + notarized on Mac), `.msi`/`.exe` on Windows, AppImage on Linux.
- CI: add release workflow that produces signed artifacts on tag push.

## Known quirks to keep in mind

- **One-frame lag on canvas.** `Canvas::render` uses last frame's camera and size. Invisible at 60 fps but technically a frame behind.
- **Colour space.** vello renders into linear `Rgba8Unorm`; egui samples it as-is. Colours may look slightly off vs. a pure-sRGB pipeline. Defer until it's a real problem; likely mitigated by a `TextureBlitter`-based conversion later.
- **Menu bar on Linux is in-window** by design (see `docs/overview.md`). `muda` is target-cfg'd out on Linux so we don't pull GTK.
- **Deprecation warnings** from egui 0.33's Panel API (`TopBottomPanel`, `SidePanel`, `.show()`) — still works, migrate when convenient. Do not suppress with `-Dwarnings` in CI while the migration is pending.

## Pointers

- Design decisions: `docs/overview.md`, `docs/stack.md`, `docs/features.md`.
- Plan lives in this file (`docs/status.md`); also mirrored in per-project Claude memory for the author's local machine.
