# youeye — status & next steps

Running log of where the project is and what's next. Update after each working session so any machine (or a fresh Claude session) can pick up cold.

Last updated: **2026-04-20**.

## Current phase

**Phase 1 — Foundation** ✅ complete.
**Phase 2 — Document model + SVG round-trip** 🚧 in progress (slices A+B landed, slice C next).

- 1a — Workspace scaffold, egui + winit + wgpu window with chrome (toolbar, layers, inspector, status bar, menu bar).
- 1b — Vello canvas with pan/zoom, rendered into an offscreen `Rgba8Unorm` texture registered as an egui native texture.
- 1c — wgpu device limits: request `adapter.limits()` (not `downlevel_defaults`) so Retina-class surfaces and vello's compute shader (5 storage buffers) are supported.
- 2A — `youeye-doc`: `Node` enum (Group/Frame/Rect/Ellipse/Path/Text) with `NodeBase` carrying id, `kurbo::Affine`, fill/stroke, `youeye_attrs`, `extra_attrs`. `Document` with `ViewBox`, tokens, variables, `raw_style`. 8 unit tests.
- 2B — `youeye-io`: SVG parser + serializer (`quick-xml` 0.39). **Canonical** round-trip — first save normalises (sorted attrs, 2-space indent, `\n`, CDATA-wrapped style, self-closing empties); every subsequent load+save is byte-identical. `<style>` content round-trips verbatim via `Document.raw_style`; tokens/variables are a read-only view extracted from `:root` declarations. Unknown attrs preserved in `extra_attrs`. 13 unit tests.

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

**Slice C (next)** — wire-up.
5. **`youeye-app`: project I/O.** Folder-as-project (`project.yeye.json` + `screens/*.svg` + `components.svg` + `assets/`). Hook into File > New / Open / Save menu actions.
6. **`youeye-app`: layers panel.** Bind to the current screen's node tree; selection, rename, reorder, visibility toggle, lock.
7. **`youeye-render`: scene builder.** Walk the doc tree, emit a `vello::Scene`. Replace the hardcoded test content in `canvas.rs::build_scene`.

**Known slice-B scope deferrals (pick up before slice C or as needed):**
- `Frame` node type parses as `Node::Group` for now. Serializer emits `Frame` as `<g>` plus `youeye:frame="true"` + `youeye:{x,y,width,height}` — a placeholder until Phase 3's auto-layout code decides the canonical encoding (candidates: nested `<svg>` vs `<g>` + namespaced attrs).
- `transform=""` attrs are preserved as raw strings in `extra_attrs` and re-emitted unchanged. Typed `NodeBase.transform: kurbo::Affine` is populated to IDENTITY on parse; the editor will populate it from manipulation and serializer falls back to `extra_attrs["transform"]` when Affine is identity.
- Paint parsing covers `none`, `#rgb/#rrggbb/#rrggbbaa`, `rgb(...)`/`rgba(...)`. Named colours (`red`, `currentColor`), gradients, `url(#id)`, `var(--token-...)` pass through as `Paint::Raw(verbatim)`.

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
