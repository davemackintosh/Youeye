# youeye — feature scope

## In scope

- **System font reading** via `parley` + `fontique`.
- **Font import** at two levels:
  - *Project-embedded:* base64-inlined in `<style>@font-face {...}</style>` in the SVG. Keeps SVG self-contained (matches Figma's SVG export).
  - *User library:* custom fonts in the app data dir (via `directories`), available across all projects.
- **Auto-layout** via `taffy` (flexbox). Stored as `youeye:layout="flex"` plus related namespaced attrs.
- **Reusable components** via SVG `<symbol>` + `<use>`, with `youeye:override-*` attrs for instance overrides (text, color, variant).
- **SVG import/export:**
  - Our own SVGs round-trip perfectly (all `youeye:*` preserved).
  - Foreign SVGs: best-effort via `usvg`, flattened into raw shapes.
- **Multiple projects** — project = folder. Default shape:
  - `project-name/project.yeye.json` — editor-only metadata (screen order, thumbnails cache).
  - `project-name/screens/*.svg` — one file per screen.
  - `project-name/components.svg` — shared components, referenced via `<use href="components.svg#id"/>`.
  - `project-name/assets/fonts/`, `assets/images/` — binary deps not inlined in SVG.
  - Zip-bundle (`.yeye`) is a later *export* format, not the primary.
- **Multiple screens per project** — falls out of folder structure.
- **Layers panel** — tree view of the current screen's node graph.

## Distinctive features

### Rulers as first-class design entities (Fusion 360 style)

- Rulers are **nodes in the document**, not editor chrome. Stored as `<line youeye:type="ruler" style="display:none" .../>` — valid SVG, invisible to foreign renderers, preserved round-trip.
- **Hierarchical:** rulers can exist at any level (global, per-screen, per-frame). A "safe-area" ruler inside a phone-frame is scoped to that frame.
- **Constraint solver required:** `kiwi` (Rust Cassowary port) handles ruler-based and pin-to-edge constraints.
- **Coexists with `taffy` flex:** taffy runs first for flex frames, kiwi runs second for non-flex constraints. Inside a flex container, flex always wins.

### Parametric design system: tokens + variables + modes + rhythm

All four layers are stored as **native CSS** in the SVG `<style>` block. No proprietary metadata. The SVG file itself is the design-system contract; implementers read CSS vars directly out of it.

- **Tokens** = atomic named values. Colors, font families, font sizes, weights, radii, opacities. `--token-brand-primary: #0052cc;`. A token *is* the value.
- **Palettes** = logically grouped sets of tokens in the UI (`brand/`, `gray/`, `accent/`). Underlying CSS is flat; grouping is an editor convention, stored in `project.yeye.json`.
- **Variables** = parametric / modal / derived. Can reference tokens, can be `calc()` expressions, can change by mode. `--var-rhythm: 8px; --var-padding-default: calc(2 * var(--var-rhythm));`
- **Modes** = CSS media queries (`@media (prefers-color-scheme: dark)`) plus CSS class modifiers on root (`.mode-compact { --var-rhythm: 4px; }`). Light/dark is free and browser-native. Custom modes use class modifiers.
- **Rhythm** is a privileged variable: `--var-rhythm`. Auto-layout gap/padding pickers default to multiples of rhythm; entering raw values requires explicit escape. Typography line-heights snap to rhythm in rhythm-locked contexts.
- **`youeye:gap`** stores `var(--var-spacing-md)` (or whatever variable) so the indirection is preserved round-trip — auto-layout gap isn't native SVG.

**Enforcement is in the UI, not the file.** SVG is always valid with raw values; editor shows an "off-token" / "off-rhythm" warning chip in the inspector if the file is hand-edited. Goal is to guide, not gatekeep.

**Editor UI:**

- Two separate panels: **Tokens** and **Variables**. Do not merge.
- Every picker (color / size / font / spacing) shows tokens/vars first, raw value as escape hatch.
- Mode switcher in top bar toggles between defined modes for preview.
- Variables panel has a small expression editor with autocomplete for token names.

Conceptually this is Fusion 360's parametric design model (parameters, derived dimensions, constraints) applied to UI design, expressed entirely in native CSS so the SVG artifact IS the design-system spec.

## Explicitly out of scope (do not re-propose)

- **Grid / snap-to-grid.** Flex is the industry standard.
- **Prototyping / hotspot links between screens.** Development teams don't use them.
- **Plugin system.** SVG is the extension point. Living SVG = AI-readable by construction.
- **Keyboard-shortcut customization.** Pick the near-universal Figma/Sketch/Illustrator set and hardcode.

## Hardcoded keyboard shortcut set

- Global: ⌘/Ctrl + `S` save, `Z` undo, `Shift+Z` redo, `C` / `V` / `X` copy/paste/cut, `A` select all, `G` group, `Shift+G` ungroup, `D` duplicate, `0` zoom-to-fit, `1` zoom-100%.
- Tools: `V` select, `R` rect, `O` ellipse, `L` line, `T` text, `P` pen, `F` frame, `H` hand.
- Canvas: `Space+drag` pan, `⌘/Ctrl+scroll` zoom, arrows nudge (`Shift+arrow` = larger nudge), `Esc` deselect.
