# Moush — Brand Identity & Application Icon Specification

> Production-Grade Windows 11 Fluent Design Application Icon & Visual Identity for **Moush** (Rust-powered seamless multi-device control).

---

## 1. Design Concept & Philosophy

The official brand identity for **Moush** is built around **Concept B — The Infinity Glide**:

1. **The Infinity Continuum (Frictionless Multi-Computer Flow)**
   - Represents the core product experience: input never "stops" at a monitor bezel; it glides continuously across computing environments.
   - Dual continuous loops unify the Host machine (left domain) and the Client machine (right domain) into one seamless plane.

2. **The Forward Vector Arrow (Sovereign Transit Apex)**
   - The right loop culminates dynamically into a 45° forward-pointing cursor arrow.
   - Signifies instant, zero-latency motion heading into the next workstation.
   - Clean vector specular facet highlight on the arrowhead for optical definition.

3. **No-Shadow Vector Discipline**
   - **Zero Filter Blurs:** Strictly `0` `feDropShadow` and `0` `feGaussianBlur` filters.
   - Every boundary, edge, and transition is achieved through pure vector curvature, mathematical geometry, and high-contrast color stops.
   - Ensures razor-sharp rendering on dense high-DPI displays, low-resolution system trays, and print applications.

4. **Fluent Squircle Container**
   - Deep Midnight Slate base (`#0F172A` → `#020408`) with an acrylic specular rim highlight (`#38BDF8` → `#818CF8`).
   - Sits natively in the Windows 11 Start Menu, Taskbar, and notification tray.

---

## 2. Geometric Construction & Grid Specifications

- **Canvas Dimensions:** `1024 × 1024 px` master unit (integer-divisible by 512, 256, 128, 64, 32, 16).
- **Fluent Squircle Tile:** `880 × 880 px` (`x="72" y="72"` with `rx="208" ry="208"`).
- **Safe Margins:** `72 px` perimeter (7.0%) padding on all four sides.
- **Infinity Ribbon Center:** Centered symmetrically along both horizontal and vertical axes.
- **Angle Alignment:** Snap angles at exact 0°, 45°, and 90° intervals.

---

## 3. Official Color Palette

| Token | Hex | Role | Usage |
|---|---|---|---|
| **Midnight Obsidian Base** | `#020408` – `#0F172A` | Background Tile | Squircle container & dark theme shell |
| **Electric Cyan** | `#00F0FF` | Primary Flow Origin | Left loop / Host domain start |
| **Sky Blue** | `#38BDF8` | Flow Mid-tone | Crossing threshold / bridge |
| **Royal Azure Blue** | `#2563EB` | Core Energy | Right loop / Client domain body |
| **Vivid Indigo** | `#6366F1` | Convergence Accent | Right loop terminus & arrow base |
| **Luminous Specular White** | `#FFFFFF` | Sheen Highlight | Arrowhead specular vector facet |

---

## 4. Asset Manifest & Deliverables

### Vector Master Files (`assets/branding/`)
- `mouse-share-app-icon.svg` — Master Windows 11 Fluent Squircle application icon (No shadows).
- `mouse-share-standalone.svg` — Transparent standalone symbol for headers, website & docs.
- `mouse-share-small.svg` — Small-size cut optimized for 16–32 px viewports.
- `mouse-share-mono-light.svg` — Pure monochrome white for dark high-contrast backgrounds.
- `mouse-share-mono-dark.svg` — Pure monochrome obsidian black for light backgrounds & print.

### Application Icon Suite (`src-tauri/icons/`)
- `icon.ico` — Multi-resolution Windows application icon (16, 24, 32, 48, 64, 128, 256 px).
- `icon.png` — 512×512 master transparent PNG.
- `32x32.png`, `64x64.png`, `128x128.png`, `128x128@2x.png` — Standard desktop sizes.
- Windows Store / Appx Tiles (`StoreLogo.png`, `Square44x44Logo.png`, `Square150x150Logo.png`, `Square310x310Logo.png`).
- `icon.icns`, iOS & Android catalog suites.

### In-App Component (`src/components/Logo.tsx`)
- Inline optimized SVG with dynamic gradient IDs matching Concept B geometry.
