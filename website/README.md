# Moush docs site

A self-contained static site (no build step) for the Moush install and usage guide.

```
website/
├── index.html                 the whole guide (features, install, usage, troubleshooting, build)
├── styles.css                 light/dark theme matching the app's design tokens
└── assets/
    ├── icon.png               app icon (favicon + brand)
    └── screenshots/*.png      captured from the UI preview mock
```

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
#   ?mockRole=host|client|none   ?theme=light|dark   &expand=all   &motion=reduce
agent-browser open "http://localhost:1420/?page=home&theme=light&motion=reduce"
agent-browser set viewport 1280 800
agent-browser wait 2200
agent-browser eval "document.querySelector('.nav-mock')?.remove()"
agent-browser screenshot "website/assets/screenshots/home.png"
```

`&motion=reduce` keeps transition animations out of the captures and the `eval` hides the
dev-only *Preview data* panel.

## Publish (GitHub Pages)

The workflow at `.github/workflows/pages.yml` uploads `website/` as the Pages artifact on
every push to `main` that touches this folder.

1. On GitHub open **Settings → Pages** and set **Build and deployment → Source** to
   **GitHub Actions** (one-time).
2. Push to `main` or run the **Docs site** workflow from the Actions tab.

The site is then served at `https://<owner>.github.io/moush/`.
