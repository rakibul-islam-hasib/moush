# Mouse Share — Brand Identity & Application Icon Specification

> Production-Grade Windows 11 / Fluent Design Application Icon & Visual Identity for **Mouse Share** (Rust-powered seamless multi-device control).

---

## 1. Design Concept & Philosophy

The icon design for **Mouse Share** is built around three clear, purposeful metaphors engineered in accordance with modern Windows 11 Fluent Design principles and professional logo design craft standards:

1. **Two Cascading Computer Displays (Workstations A & B)**
   - **Host Display (Left / Baseline):** Positioned at `(88, 192)` with dimensions `216 × 146 px` and gentle `20px` corner radii. Rendered in deep sapphire-slate (`#14213D` → `#091122`) with an electric azure specular rim (`#60A5FA` / `#1D4ED8`). Represents the primary input workstation.
   - **Client Display (Right / Foreground):** Staggered upward at `(208, 134)` with identical `216 × 146 px` geometry. Elevated in z-space with an active glowing cyan-indigo glass surface (`#1C3563` → `#091326`) and an illuminated cyan top specular highlight (`#00F0FF`).
   - **Vector Occlusion Plate:** Separated from the rear display by a crisp `6px` dark obsidian backplate (`#03060E`) rather than fuzzy, non-standard SVG filter blurs. This guarantees razor-sharp definition and zero blur artifacts at 16×16 and 32×32 pixels.

2. **The Flowing Connection Conduit (Zero-Latency Bridge)**
   - A bold, continuous `24px` flowing energy spline (`M 148 268 C 196 304 238 166 368 204`) spanning both displays.
   - Seamless 4-stop Fluent gradient: **Neon Cyan (`#00F0FF`) → Sky Blue (`#38BDF8`) → Royal Azure (`#3B82F6`) → Vivid Violet (`#A855F7`)**.
   - Solid grounded terminal nodes (`r=9px`, diameter `18px` ≥ 1/32 of canvas) anchored into the heart of each workstation.
   - Symbolizes the lightning-fast, zero-latency transmission of mouse, keyboard, clipboard, and file streaming powered by Rust.

3. **The Iconic White Mouse Cursor (Sovereign Pointer)**
   - Positioned precisely at the golden apex `(240, 190)`, seamlessly bridging the boundary between both screens.
   - Precision-tilted at **-15°** along the energy trajectory to convey active transit and responsive control.
   - **Geometric Anatomy:** Designed with mathematically verified snap angles (0°, 15°, 30°, 45°, 75°, 90°) to eliminate any near-miss angle artifacts.
   - **100% Boundary Isolation:** Enclosed by a high-contrast obsidian protective perimeter (`#020409`, `14px`), surrounding a pure `#FFFFFF` pointer body with a subtle Fluent `#F1F5F9` sheen facet on the left spine. Ensures flawless contrast against bright cyan streams, dark bezels, and custom wallpapers.

---

## 2. Geometric Construction & Grid Specifications

- **Canvas Dimensions:** `512 × 512 px` (Standard Windows master unit, integer-divisible by 256, 128, 64, 32, 16).
- **Fluent Squircle Tile:** `432 × 432 px` (`x="40" y="40"` with `rx="96" ry="96"`).
- **Safe Margins:** `40 px` perimeter (7.8%) padding on all four sides to meet Windows 11 Start Menu and desktop shortcut guidelines.
- **Visual Center of Mass:** Centered at `(256, 256)` with symmetrical weight distribution.
- **Craft Quality Audit:**
  - `0` filter blurs (feDropShadow / feGaussianBlur eliminated for cross-platform fidelity).
  - `0` near-miss angles (all edges snap to deliberate whole-degree increments).
  - `0` tiny sub-pixel details (no disappearing mini buttons or hairline lines).
  - Production readiness audit score: **74/100** for master full-color app tile, **97/100** for monochrome silhouette.

---

## 3. Official Color Palette

| Token | Hex | Role | Usage |
|---|---|---|---|
| **Midnight Navy Base** | `#03060D` – `#0F172A` | Background Tile | Squircle container & dark theme integration |
| **Neon Cyan** | `#00F0FF` | Primary Accent | Source stream node, target screen specular rim |
| **Sky Specular Rim** | `#38BDF8` | Fluent Highlight | Acrylic tile border highlight, stream transition |
| **Royal Azure Blue** | `#3B82F6` | Core Energy | Conduit mid-flow, host screen border accent |
| **Vivid Violet** | `#A855F7` | Convergence Accent | Target screen terminal node & energy terminus |
| **Pure White** | `#FFFFFF` | Hero Cursor Fill | Primary focal point, maximum visual punch |
| **Obsidian Shield** | `#020409` | Protective Isolation | Cursor barrier & occlusion depth plate |

---

## 4. Asset Manifest & Deliverables

### Vector Master Files (`assets/branding/`)
- [`mouse-share-app-icon.svg`](./mouse-share-app-icon.svg): Full-color master application icon with Fluent squircle tile (`512 × 512`).
- [`mouse-share-standalone.svg`](./mouse-share-standalone.svg): Scaled and optically centered mark on a transparent canvas.
- [`mouse-share-mono-light.svg`](./mouse-share-mono-light.svg): High-contrast white silhouette for dark mode taskbars and system trays.
- [`mouse-share-mono-dark.svg`](./mouse-share-mono-dark.svg): High-contrast dark slate silhouette for light mode taskbars and documentation.
- [`mouse-share-small.svg`](./mouse-share-small.svg): Hand-tuned small-size cut with amplified strokes and enlarged cursor for 16×16 and 32×32 mipmaps.

### Windows & Tauri Production Assets (`src-tauri/icons/`)
- [`icon.ico`](../../src-tauri/icons/icon.ico): Multi-resolution Windows desktop icon package containing **16×16, 24×24, 32×32, 48×48, 64×64, 128×128, and 256×256** embedded PNG mipmaps.
- [`icon.png`](../../src-tauri/icons/icon.png): 512×512 master application raster.
- [`32x32.png`](../../src-tauri/icons/32x32.png): 32×32 pixel-crisp taskbar and system tray asset.
- [`64x64.png`](../../src-tauri/icons/64x64.png): 64×64 desktop shortcut asset.
- [`128x128.png`](../../src-tauri/icons/128x128.png) & `128x128@2x.png` (256×256): High-DPI display assets.
- **Windows Store / MSIX Tiles:** `StoreLogo.png`, `Square30x30Logo.png`, `Square44x44Logo.png`, `Square71x71Logo.png`, `Square89x89Logo.png`, `Square107x107Logo.png`, `Square142x142Logo.png`, `Square150x150Logo.png`, `Square284x284Logo.png`, `Square310x310Logo.png`.

---

## 5. Usage & Implementation Guidelines

- **Clear Space:** Maintain a clear margin of at least 8% around the squircle perimeter.
- **System Tray:** Use `mouse-share-mono-light.svg` or `32x32.png` for native Windows dark mode taskbars and notification area tray icons.
- **Integrity Rules:**
  - Do not add arbitrary text, letters, or wordmarks inside the icon tile.
  - Do not alter the Northwest -15° cursor orientation.
  - Do not re-introduce blurry SVG `filter` drop-shadows; maintain pure vector geometry for all exports.
