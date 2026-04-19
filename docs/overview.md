# youeye — project overview

youeye is a cross-platform, **offline-first** vector UI design application — Figma/Penpot/Sketch-class. Greenfield Rust project, started 2026-04-19.

## Hard constraints

- **SVG is the source of truth.** No proprietary file format. Save/open/import are all SVG.
- **Flexbox is the auto-layout target.** Figma-style auto-layout maps to flexbox; `taffy` is the planned engine.
- **Entirely offline. No online or collab features — ever.** Don't add CRDT shaping, sync scaffolding, or "future online" hooks.
- **Linux-first, but Mac and Windows are near-term targets** (daily-driver platforms for the author). Cross-platform hygiene is baked in from day one, not retrofitted:
  - Abstract modifier keys (⌘ vs Ctrl).
  - Use `std::path::Path` and the `directories` crate for per-OS data dirs.
  - Force `\n` line endings in SVG output.
  - Run CI on all three OSes from the first commit.
- **Native menus from day one** — Blender's in-window menu on Mac is the anti-pattern to avoid.
  - **Mac + Windows:** `muda` (Tauri team's cross-platform native menu crate) integrated into the winit event loop. On Mac, populate the app menu (About / Preferences / Services / Hide / Quit) in the correct order per Apple HIG.
  - **Linux:** egui's in-window menu bar. Linux has no consistent menu-bar convention across DEs and the GTK dep isn't worth the ambiguity. Abstract the menu layer so the platform split lives in one file.
- **Distribution:** Apple Developer account in hand; Mac builds will be code-signed (Developer ID Application) and notarized under hardened runtime, **not sandboxed** (design tools conventionally aren't — Sketch/Figma aren't sandboxed either). Use `cargo-packager`. Target `aarch64-apple-darwin` only until Intel support is explicitly needed. Windows signing (Azure Trusted Signing or EV cert) is a separate follow-on. Full distribution pipeline is deferred to a later phase but the build shouldn't be structured in a way that blocks it.
- **Rendering should match implementation reality** — what a developer building the designed UI would actually ship (browser/native).

## Non-SVG editor state

Auto-layout, components, variants, and other non-native-SVG editor state are stored as a custom XML namespace (`youeye:layout="flex"` etc). The file stays valid SVG and round-trips in the editor. Prior art: Inkscape's `inkscape:*` and Sketch. **Never bake to absolute positions on save.**

## Why

The author wants a design tool where the file format is universally interoperable (SVG), the layout model matches what implementers actually use (flexbox), and there's no vendor lock-in or cloud dependency.
