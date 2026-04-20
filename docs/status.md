# youeye — status & next steps

Running log of where the project is and what's next. Update after each working session so any machine (or a fresh Claude session) can pick up cold.

Last updated: **2026-04-20**.

## Current phase

**Phase 1 — Foundation** ✅ complete.
**Phase 2 — Document model + SVG round-trip** ✅ complete (slices A, B, C).
**Phase 3 — Auto-layout (taffy)** ✅ complete (slices A, B, C).
**Phase 4 — Tokens + variables UI with enforcement** 🚧 slices A+B+C done, D deferred.

- 1a — Workspace scaffold, egui + winit + wgpu window with chrome (toolbar, layers, inspector, status bar, menu bar).
- 1b — Vello canvas with pan/zoom, rendered into an offscreen `Rgba8Unorm` texture registered as an egui native texture.
- 1c — wgpu device limits: request `adapter.limits()` (not `downlevel_defaults`) so Retina-class surfaces and vello's compute shader (5 storage buffers) are supported.
- 2A — `youeye-doc`: `Node` enum (Group/Frame/Rect/Ellipse/Path/Text) with `NodeBase` carrying id, `kurbo::Affine`, fill/stroke, `youeye_attrs`, `extra_attrs`. `Document` with `ViewBox`, tokens, variables, `raw_style`. 8 unit tests.
- 2B — `youeye-io`: SVG parser + serializer (`quick-xml` 0.39). **Canonical** round-trip — first save normalises (sorted attrs, 2-space indent, `\n`, CDATA-wrapped style, self-closing empties); every subsequent load+save is byte-identical. `<style>` content round-trips verbatim via `Document.raw_style`; tokens/variables are a read-only view extracted from `:root` declarations. Unknown attrs preserved in `extra_attrs`. 13 unit tests.
- 2C — Wire-up. `youeye-render::build(&mut Scene, &Document, Affine)` walks the doc tree and emits vello draw commands (solid fills/strokes on rect/ellipse/path; `Paint::Raw` is a no-op for now). `Canvas::render` now takes `Option<&Document>`; app holds a `DocumentState { doc, path, dirty }`. File > Open / Save / Save As / New wired via `rfd` file dialogs (single `.svg` file per "project" for now — full folder layout deferred). Layers panel in the left sidebar shows the node tree with click-to-select (path-based selection model); Inspector panel now lists tokens/variables when a doc is open. 3 render-crate tests.

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

Sliced A/B/C for tractable commits. **Round-trip policy: canonical**, not strict byte-exact — first save normalises, subsequent load+save is byte-stable.

**Slice A ✅** — `youeye-doc` types.
1. **`youeye-doc`: node types mirroring SVG.** ✅ Group, Rect, Ellipse, Path, Text, Frame. Each carries an id, `kurbo::Affine` transform, fill/stroke, `youeye_attrs`, and `extra_attrs` for unknown-attribute preservation.
2. **`youeye-doc`: tokens + variables.** ✅ `Tokens` / `Variables` as `BTreeMap<String, String>` (bare names, prefix stripped). Parsing lives in `youeye-io`; here they're typed dictionaries.

**Slice B ✅** — `youeye-io` parse + serialize.
3. **`youeye-io`: SVG parser.** ✅ `quick-xml` 0.39-based. Preserves unknown attrs into `extra_attrs`. Routes `youeye:*` into `youeye_attrs`. Captures `<style>` text verbatim into `Document.raw_style`; extracts `--token-*`/`--var-*` out of any `:root` blocks for the inspector view. Foreign SVG via `usvg` is a later entry point, not wired yet.
4. **`youeye-io`: SVG serializer.** ✅ `\n`, 2-space indent, sorted attrs, self-closing empties, `<?xml ...?>` declaration, CDATA-wrapped `<style>`. Default `xmlns`/`xmlns:youeye` auto-added when missing. 13 round-trip/unit tests.

**Slice C ✅** — wire-up.
5. **`youeye-app`: project I/O.** ✅ *Scoped down*: single `.svg` file per "project" via `rfd` dialogs (File > New / Open / Save / Save As). Full folder-as-project layout (`project.yeye.json` + `screens/*.svg` + `components.svg` + `assets/`) deferred — pick up when we need multi-screen or component sharing.
6. **`youeye-app`: layers panel.** ✅ Tree view of current document bound to `doc.children`. Selection as `Vec<usize>` path. Rename / reorder / visibility toggle / lock are Phase 6 concerns.
7. **`youeye-render`: scene builder.** ✅ `youeye_render::build(&mut Scene, &Document, Affine)` — solid fills/strokes on rect/ellipse/path; `Paint::Raw` silently skipped. Canvas keeps grid + crosshair as background decoration.

**Known slice-B scope deferrals (pick up before slice C or as needed):**
- `Frame` node type parses as `Node::Group` for now. Serializer emits `Frame` as `<g>` plus `youeye:frame="true"` + `youeye:{x,y,width,height}` — a placeholder until Phase 3's auto-layout code decides the canonical encoding (candidates: nested `<svg>` vs `<g>` + namespaced attrs).
- `transform=""` attrs are preserved as raw strings in `extra_attrs` and re-emitted unchanged. Typed `NodeBase.transform: kurbo::Affine` is populated to IDENTITY on parse; the editor will populate it from manipulation and serializer falls back to `extra_attrs["transform"]` when Affine is identity.
- Paint parsing covers `none`, `#rgb/#rrggbb/#rrggbbaa`, `rgb(...)`/`rgba(...)`. Named colours (`red`, `currentColor`), gradients, `url(#id)`, `var(--token-...)` pass through as `Paint::Raw(verbatim)`.

### Phase 3 — Auto-layout (taffy)

Sliced A/B/C like phase 2. **Decision:** Frame is a nested `<svg>` on wire — its own viewport, native clipping, local coordinate space for children. Foreign renderers display children clipped to the frame bounds automatically. No `youeye:type` marker needed; any non-root `<svg>` is a Frame.

**Slice A ✅** — Frame SVG representation.
- Parser: non-root `<svg>` → `Node::Frame`; `x`/`y`/`width`/`height` extracted to typed fields, `youeye:*` flows into `youeye_attrs`. Empty `<svg />` supported.
- Serializer: Frame → `<svg x=".." y=".." width=".." height="..">`; flex-layout metadata rides through as `youeye:*`.
- 5 new tests: parse, round-trip, empty frame, flex-attr preservation, nested frames. Full io suite: 18 tests.

**Slice B ✅** — Taffy integration.
- `taffy` 0.10 pinned in workspace, dep on `youeye-render`.
- New `youeye_render::layout` module — `compute_flex_positions(&Frame) -> Option<Vec<ChildLayout>>`. Scene builder translates each child so its authored top-left lands at taffy's computed position, regardless of the child's authored `(x, y)`.
- Frame-level attrs read today: `layout="flex"`, `flex-direction`, `justify`, `align`, `gap`, `padding`. Single-value padding only (shorthand parsing defers to slice C).
- `var(--...)` / `calc(...)` on gap/padding resolve to `0` for now; token resolver lands with the inspector in slice C.
- Child size heuristics: Rect = `(width, height)`; Frame = `(width, height)`; Ellipse = `(2rx, 2ry)`; Path = kurbo bounding box; Group / Text contribute `0x0` until their intrinsic sizing lands.
- Per-child flex overrides (`flex-grow`, `flex-shrink`, `align-self`, `flex-basis`) still deferred.
- 9 layout unit tests + 3 existing scene tests; render suite 12 passing.

**Slice C ✅** — Inspector write-back.
- `Document::node_at` / `node_at_mut` path-based lookup (4 new tests).
- `ui.draw` and `state.render` now take `Option<&mut DocumentState>`. Canvas still gets `&Document` via an inner scope that releases the immutable borrow before the egui closure captures the mutable one (`Option::as_deref_mut()` inside the FnMut closure).
- Inspector renders flex controls when the selection is a Frame: "Auto layout (flex)" toggle, direction / justify / align combos, gap / padding DragValues. Gap/padding DragValue step defaults to `--var-rhythm` when present, else `1.0`. Writes update `frame.base.youeye_attrs` and mark the doc dirty.
- Inspector also shows tokens/variables for the current doc regardless of selection.

### Phase 4 — Tokens + variables UI with enforcement

**Slice A ✅** — Token/Variable CRUD.
- Flip ownership: `Document.tokens`/`variables` are now authoritative; `raw_style` renamed to `raw_style_extra` and only stores non-`:root` CSS (`@font-face`, etc.).
- Parser: `split_style_block(css)` extracts `:root` declarations into Tokens/Variables and puts the rest into `raw_style_extra`.
- Serializer: regenerates a canonical `:root { ... }` block from Tokens/Variables (sorted) and appends `raw_style_extra` verbatim. First save canonicalises; subsequent load+save byte-stable.
- Inspector: tokens/variables panels are now editable — rename, change value, delete, add. Shared `draw_dict_editor` helper used for both. Changes mark doc dirty.
- 4 new io tests (extra-preserved alongside tokens, extra-only, editing-reflects-in-output, and the existing split tests).

**Slice B ✅** — Token-first pickers.
- Rect/Ellipse/Path selections now show Fill and Stroke pickers in the inspector. Each picker offers modes: `none` / `color` / `token` / `raw`.
- `color` uses egui's `color_edit_button_rgba_unmultiplied` on the typed `Color { r, g, b, a: f32 }`.
- `token` shows a dropdown of `--token-*` names; picking writes `Paint::Raw("var(--token-xxx)")`.
- `raw` is a plain text field — for named colours, `url(#grad1)`, or anything not yet typed.
- Stroke gains a width DragValue when paint isn't `none`.

**Slice C ✅** — Off-token / off-rhythm chips.
- Small amber "off-token" label next to fill/stroke when the doc has tokens but the paint is raw.
- "off-rhythm" label next to gap/padding when the doc has `--var-rhythm` and the current value isn't a rhythm multiple. Tooltip suggests the nearest rhythm-aligned value.
- Non-enforcing; design is "guide, don't gatekeep."

**Slice D** (deferred) — Mode switcher.
- `@media (prefers-color-scheme: dark)` and class modifiers (`.mode-compact { ... }`) — needs CSS context evaluation and mode-aware token/variable lookup. Defer until there's a concrete use case.

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
