# Moush website

A self-contained static landing page (no build step): what Moush does, an interactive demo,
features, security, performance, setup and FAQ.

```
website/
├── index.html                 the page (icons are an inline sprite of the app's icon set)
├── styles.css                 dark/light theme in the app's design language (Geist, tonal, no shadows)
├── app.js                     theme toggle, copy buttons and the interactive desk demo
└── assets/
    ├── icon.svg, icon.png     app icon (favicon + brand)
    ├── screenshots/*.webp     2x captures from the UI preview mock, one per theme
    └── video/                 launch film (MP4) and poster (JPG), credited in THIRD_PARTY_NOTICES.md
```

Every number and claim on the page comes from `README.md` and `docs/` (PERFORMANCE.md,
SECURITY.md, GUIDE.md); update the page when those change.

## Preview locally

Open `index.html` in a browser, or serve the folder:

```powershell
npx vite website                      # serves the folder on http://localhost:5173
# or simply: start website\index.html
```

## Screenshots

The screenshots were captured from the app's browser preview (`npm run dev`), which is the
mock backend used for UI work. Every screen is reachable through URL switches, so they can
be regenerated deterministically:

```powershell
npm run dev
# then, in another shell (see src/lib/dev.ts and src/api/mock.ts):
#   ?page=home|devices|layout|features|transfers|settings|diagnostics
#   ?mockRole=host|client|none   ?theme=light|dark   &expand=all   &motion=reduce   &clean=1
& "C:\Program Files\Google\Chrome\Application\chrome.exe" --headless=new --hide-scrollbars `
  --force-device-scale-factor=2 --window-size=1280,800 --virtual-time-budget=5000 `
  --screenshot=home-dark.png "http://localhost:1420/?page=home&theme=dark&motion=reduce&clean=1"
```

Capture each screen in `dark` and `light`, then convert to WebP (quality ~86). `&motion=reduce`
keeps transitions out of the captures and `&clean=1` hides the dev-only *Preview data* panel.
The `crop-*` images are cut from the layout and transfers captures.

## Publish (GitHub Pages)

The workflow at `.github/workflows/pages.yml` uploads `website/` as the Pages artifact on
every push to `main` that touches this folder.

1. On GitHub open **Settings → Pages** and set **Build and deployment → Source** to
   **GitHub Actions** (one-time).
2. Push to `main` or run the **Docs site** workflow from the Actions tab.

The site is then served at `https://<owner>.github.io/moush/`.
