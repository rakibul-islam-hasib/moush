import os
import subprocess
from PIL import Image

# Directories
BRANDING_DIR = "assets/branding"
ICONS_DIR = "src-tauri/icons"
os.makedirs(BRANDING_DIR, exist_ok=True)
os.makedirs(ICONS_DIR, exist_ok=True)

# -----------------------------------------------------------------------------
# 1. Master Application Icon (With Windows 11 Fluent Squircle Container)
# -----------------------------------------------------------------------------
SVG_APP_ICON = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <!-- Background Midnight Navy Gradient -->
    <linearGradient id="bg-grad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0C1527"/>
      <stop offset="45%" stop-color="#080E1B"/>
      <stop offset="100%" stop-color="#04060C"/>
    </linearGradient>

    <!-- Acrylic Squircle Border Highlight (Fluent Glass Rim) -->
    <linearGradient id="tile-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#38BDF8" stop-opacity="0.45"/>
      <stop offset="30%" stop-color="#818CF8" stop-opacity="0.25"/>
      <stop offset="70%" stop-color="#1E293B" stop-opacity="0.10"/>
      <stop offset="100%" stop-color="#0F172A" stop-opacity="0.05"/>
    </linearGradient>

    <!-- Radial Ambient Backlight -->
    <radialGradient id="ambient-glow" cx="50%" cy="46%" r="52%">
      <stop offset="0%" stop-color="#0284C7" stop-opacity="0.26"/>
      <stop offset="45%" stop-color="#4F46E5" stop-opacity="0.14"/>
      <stop offset="85%" stop-color="#04060C" stop-opacity="0"/>
    </radialGradient>

    <!-- Left Screen (Machine 1 - Primary Source) -->
    <linearGradient id="s1-frame" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#1D283C"/>
      <stop offset="100%" stop-color="#0E1626"/>
    </linearGradient>
    <linearGradient id="s1-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#60A5FA" stop-opacity="0.45"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.12"/>
    </linearGradient>
    <linearGradient id="s1-glass" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0D2248"/>
      <stop offset="60%" stop-color="#08152E"/>
      <stop offset="100%" stop-color="#050E20"/>
    </linearGradient>

    <!-- Right Screen (Machine 2 - Receiving Target, Higher Elevation) -->
    <linearGradient id="s2-frame" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#263852"/>
      <stop offset="100%" stop-color="#121D30"/>
    </linearGradient>
    <linearGradient id="s2-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#38BDF8" stop-opacity="0.80"/>
      <stop offset="50%" stop-color="#818CF8" stop-opacity="0.45"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.15"/>
    </linearGradient>
    <linearGradient id="s2-glass" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#12366E"/>
      <stop offset="50%" stop-color="#0E244C"/>
      <stop offset="100%" stop-color="#07142E"/>
    </linearGradient>

    <!-- Flowing Stream Core Gradient (Neon Cyan -> Blue -> Indigo -> Violet -> Magenta) -->
    <linearGradient id="stream-core" x1="0%" y1="60%" x2="100%" y2="40%">
      <stop offset="0%" stop-color="#00F0FF"/>
      <stop offset="25%" stop-color="#0284C7"/>
      <stop offset="50%" stop-color="#3B82F6"/>
      <stop offset="75%" stop-color="#6366F1"/>
      <stop offset="90%" stop-color="#A855F7"/>
      <stop offset="100%" stop-color="#D946EF"/>
    </linearGradient>

    <!-- Flowing Stream Ambient Glow Gradient -->
    <linearGradient id="stream-glow" x1="0%" y1="60%" x2="100%" y2="40%">
      <stop offset="0%" stop-color="#00E5FF"/>
      <stop offset="50%" stop-color="#38BDF8"/>
      <stop offset="100%" stop-color="#A855F7"/>
    </linearGradient>

    <!-- Fluent Elevation Shadows -->
    <filter id="shadow-s2" x="-35%" y="-35%" width="170%" height="180%">
      <feDropShadow dx="-16" dy="20" stdDeviation="24" flood-color="#000000" flood-opacity="0.72"/>
    </filter>

    <filter id="shadow-cursor" x="-35%" y="-35%" width="170%" height="170%">
      <feDropShadow dx="-4" dy="12" stdDeviation="14" flood-color="#000000" flood-opacity="0.85"/>
    </filter>

    <filter id="blur-glow" x="-25%" y="-50%" width="150%" height="200%">
      <feGaussianBlur stdDeviation="14" result="blur"/>
      <feComposite in="SourceGraphic" in2="blur" operator="over"/>
    </filter>
  </defs>

  <!-- 1. Master Windows 11 Fluent Squircle Base (864 × 864, Safe Margins 80px) -->
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="url(#bg-grad)"/>
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="url(#ambient-glow)"/>
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="none" stroke="url(#tile-rim)" stroke-width="2.5"/>

  <!-- 2. Screen 1 (Left - Workstation A) -->
  <g id="screen-left">
    <!-- Outer Bezel -->
    <rect x="196" y="356" width="374" height="268" rx="28" fill="url(#s1-frame)" stroke="url(#s1-rim)" stroke-width="2"/>
    <!-- Active Display Surface -->
    <rect x="210" y="370" width="346" height="240" rx="18" fill="url(#s1-glass)"/>
    
    <!-- Screen 1 UI / Workspace Affordances -->
    <circle cx="236" cy="392" r="4.5" fill="#38BDF8" fill-opacity="0.45"/>
    <circle cx="254" cy="392" r="4.5" fill="#818CF8" fill-opacity="0.35"/>
    <line x1="278" y1="392" x2="344" y2="392" stroke="#38BDF8" stroke-opacity="0.18" stroke-width="2" stroke-linecap="round"/>
    
    <!-- Workspace Desktop Card 1 -->
    <rect x="240" y="420" width="136" height="92" rx="10" fill="#1E3A8A" fill-opacity="0.18" stroke="#38BDF8" stroke-opacity="0.16" stroke-width="1.5"/>
    <line x1="256" y1="442" x2="320" y2="442" stroke="#60A5FA" stroke-opacity="0.25" stroke-width="2" stroke-linecap="round"/>
    <line x1="256" y1="458" x2="296" y2="458" stroke="#60A5FA" stroke-opacity="0.18" stroke-width="2" stroke-linecap="round"/>

    <!-- Bottom Dock / Taskbar Accent -->
    <line x1="210" y1="584" x2="556" y2="584" stroke="#38BDF8" stroke-opacity="0.14" stroke-width="1.5"/>
  </g>

  <!-- 3. Screen 2 (Right - Workstation B, Elevated with Ambient Shadow) -->
  <g id="screen-right" filter="url(#shadow-s2)">
    <!-- Outer Bezel -->
    <rect x="454" y="292" width="374" height="268" rx="28" fill="url(#s2-frame)" stroke="url(#s2-rim)" stroke-width="2.5"/>
    <!-- Active Display Surface -->
    <rect x="468" y="306" width="346" height="240" rx="18" fill="url(#s2-glass)"/>

    <!-- Screen 2 UI / Workspace Affordances -->
    <circle cx="494" cy="328" r="4.5" fill="#00F0FF" fill-opacity="0.6"/>
    <circle cx="512" cy="328" r="4.5" fill="#A855F7" fill-opacity="0.5"/>
    <line x1="536" y1="328" x2="602" y2="328" stroke="#60A5FA" stroke-opacity="0.25" stroke-width="2" stroke-linecap="round"/>

    <!-- Workspace Desktop Card 2 -->
    <rect x="646" y="348" width="136" height="92" rx="10" fill="#0284C7" fill-opacity="0.22" stroke="#38BDF8" stroke-opacity="0.3" stroke-width="1.5"/>
    <line x1="662" y1="370" x2="726" y2="370" stroke="#38BDF8" stroke-opacity="0.35" stroke-width="2" stroke-linecap="round"/>
    <line x1="662" y1="386" x2="702" y2="386" stroke="#38BDF8" stroke-opacity="0.25" stroke-width="2" stroke-linecap="round"/>

    <!-- Bottom Dock / Taskbar Accent -->
    <line x1="468" y1="520" x2="814" y2="520" stroke="#60A5FA" stroke-opacity="0.18" stroke-width="1.5"/>
  </g>

  <!-- 4. The Flowing Connection Stream (Seamless Conduit) -->
  <g id="flowing-conduit">
    <!-- Ambient Luminous Glow Path -->
    <path d="M 276 512 C 356 544 416 480 512 436 C 608 392 660 362 748 402" 
          fill="none" stroke="url(#stream-glow)" stroke-width="50" stroke-linecap="round" opacity="0.32" filter="url(#blur-glow)"/>
    
    <!-- Sharp Core Energy Stream -->
    <path d="M 276 512 C 356 544 416 480 512 436 C 608 392 660 362 748 402" 
          fill="none" stroke="url(#stream-core)" stroke-width="32" stroke-linecap="round"/>

    <!-- Source Connection Terminal (Screen 1) -->
    <circle cx="276" cy="512" r="9" fill="#00F0FF"/>
    <circle cx="276" cy="512" r="18" fill="none" stroke="#00F0FF" stroke-width="3.5" stroke-opacity="0.5"/>

    <!-- Target Connection Terminal (Screen 2) -->
    <circle cx="748" cy="402" r="9" fill="#D946EF"/>
    <circle cx="748" cy="402" r="18" fill="none" stroke="#D946EF" stroke-width="3.5" stroke-opacity="0.5"/>
  </g>

  <!-- 5. The Hero Mouse Cursor (Sharp, Crisp, Precision-Tilted Pointer) -->
  <!-- Positioned at the pinnacle crossing point (X=496, Y=390), tilted -10° along the flow line -->
  <g id="mouse-cursor-hero" filter="url(#shadow-cursor)" transform="translate(496, 390) rotate(-10)">
    <!-- High-Contrast Dark Protective Stroke (Ensures 100% boundary isolation against all backgrounds) -->
    <path d="M 0 0 L 0 156 L 38 122 L 78 202 L 116 184 L 78 106 L 134 106 Z" 
          fill="#050811" stroke="#050811" stroke-width="15" stroke-linejoin="round" stroke-linecap="round"/>
    
    <!-- Pure White Hero Fill -->
    <path d="M 0 0 L 0 156 L 38 122 L 78 202 L 116 184 L 78 106 L 134 106 Z" 
          fill="#FFFFFF"/>
    
    <!-- Subtle Fluent Bevel / Sheen for Tactile Polish -->
    <path d="M 0 0 L 0 126 L 28 102 L 0 0 Z" 
          fill="#F1F5F9" fill-opacity="0.85"/>
  </g>
</svg>
'''

# -----------------------------------------------------------------------------
# 2. Standalone Symbol / Glyph (Transparent Background)
# -----------------------------------------------------------------------------
SVG_STANDALONE = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <!-- Screen Gradients -->
    <linearGradient id="st-s1-frame" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#1D283C"/>
      <stop offset="100%" stop-color="#0E1626"/>
    </linearGradient>
    <linearGradient id="st-s1-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#60A5FA" stop-opacity="0.45"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.12"/>
    </linearGradient>
    <linearGradient id="st-s1-glass" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0D2248"/>
      <stop offset="60%" stop-color="#08152E"/>
      <stop offset="100%" stop-color="#050E20"/>
    </linearGradient>

    <linearGradient id="st-s2-frame" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#263852"/>
      <stop offset="100%" stop-color="#121D30"/>
    </linearGradient>
    <linearGradient id="st-s2-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#38BDF8" stop-opacity="0.80"/>
      <stop offset="50%" stop-color="#818CF8" stop-opacity="0.45"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.15"/>
    </linearGradient>
    <linearGradient id="st-s2-glass" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#12366E"/>
      <stop offset="50%" stop-color="#0E244C"/>
      <stop offset="100%" stop-color="#07142E"/>
    </linearGradient>

    <!-- Stream Gradient -->
    <linearGradient id="st-stream-core" x1="0%" y1="60%" x2="100%" y2="40%">
      <stop offset="0%" stop-color="#00F0FF"/>
      <stop offset="25%" stop-color="#0284C7"/>
      <stop offset="50%" stop-color="#3B82F6"/>
      <stop offset="75%" stop-color="#6366F1"/>
      <stop offset="90%" stop-color="#A855F7"/>
      <stop offset="100%" stop-color="#D946EF"/>
    </linearGradient>
    <linearGradient id="st-stream-glow" x1="0%" y1="60%" x2="100%" y2="40%">
      <stop offset="0%" stop-color="#00E5FF"/>
      <stop offset="50%" stop-color="#38BDF8"/>
      <stop offset="100%" stop-color="#A855F7"/>
    </linearGradient>

    <!-- Shadows -->
    <filter id="st-shadow-s2" x="-35%" y="-35%" width="170%" height="180%">
      <feDropShadow dx="-16" dy="20" stdDeviation="24" flood-color="#000000" flood-opacity="0.75"/>
    </filter>
    <filter id="st-shadow-cursor" x="-35%" y="-35%" width="170%" height="170%">
      <feDropShadow dx="-4" dy="12" stdDeviation="14" flood-color="#000000" flood-opacity="0.85"/>
    </filter>
    <filter id="st-blur-glow" x="-25%" y="-50%" width="150%" height="200%">
      <feGaussianBlur stdDeviation="14" result="blur"/>
      <feComposite in="SourceGraphic" in2="blur" operator="over"/>
    </filter>
  </defs>

  <!-- Left Screen -->
  <g id="st-screen-left">
    <rect x="196" y="356" width="374" height="268" rx="28" fill="url(#st-s1-frame)" stroke="url(#st-s1-rim)" stroke-width="2.5"/>
    <rect x="210" y="370" width="346" height="240" rx="18" fill="url(#st-s1-glass)"/>
    <circle cx="236" cy="392" r="4.5" fill="#38BDF8" fill-opacity="0.45"/>
    <circle cx="254" cy="392" r="4.5" fill="#818CF8" fill-opacity="0.35"/>
    <line x1="278" y1="392" x2="344" y2="392" stroke="#38BDF8" stroke-opacity="0.18" stroke-width="2" stroke-linecap="round"/>
    <rect x="240" y="420" width="136" height="92" rx="10" fill="#1E3A8A" fill-opacity="0.18" stroke="#38BDF8" stroke-opacity="0.16" stroke-width="1.5"/>
    <line x1="256" y1="442" x2="320" y2="442" stroke="#60A5FA" stroke-opacity="0.25" stroke-width="2" stroke-linecap="round"/>
    <line x1="256" y1="458" x2="296" y2="458" stroke="#60A5FA" stroke-opacity="0.18" stroke-width="2" stroke-linecap="round"/>
    <line x1="210" y1="584" x2="556" y2="584" stroke="#38BDF8" stroke-opacity="0.14" stroke-width="1.5"/>
  </g>

  <!-- Right Screen (Elevated) -->
  <g id="st-screen-right" filter="url(#st-shadow-s2)">
    <rect x="454" y="292" width="374" height="268" rx="28" fill="url(#st-s2-frame)" stroke="url(#st-s2-rim)" stroke-width="2.5"/>
    <rect x="468" y="306" width="346" height="240" rx="18" fill="url(#st-s2-glass)"/>
    <circle cx="494" cy="328" r="4.5" fill="#00F0FF" fill-opacity="0.6"/>
    <circle cx="512" cy="328" r="4.5" fill="#A855F7" fill-opacity="0.5"/>
    <line x1="536" y1="328" x2="602" y2="328" stroke="#60A5FA" stroke-opacity="0.25" stroke-width="2" stroke-linecap="round"/>
    <rect x="646" y="348" width="136" height="92" rx="10" fill="#0284C7" fill-opacity="0.22" stroke="#38BDF8" stroke-opacity="0.3" stroke-width="1.5"/>
    <line x1="662" y1="370" x2="726" y2="370" stroke="#38BDF8" stroke-opacity="0.35" stroke-width="2" stroke-linecap="round"/>
    <line x1="662" y1="386" x2="702" y2="386" stroke="#38BDF8" stroke-opacity="0.25" stroke-width="2" stroke-linecap="round"/>
    <line x1="468" y1="520" x2="814" y2="520" stroke="#60A5FA" stroke-opacity="0.18" stroke-width="1.5"/>
  </g>

  <!-- Flowing Stream -->
  <g id="st-conduit">
    <path d="M 276 512 C 356 544 416 480 512 436 C 608 392 660 362 748 402" 
          fill="none" stroke="url(#st-stream-glow)" stroke-width="50" stroke-linecap="round" opacity="0.35" filter="url(#st-blur-glow)"/>
    <path d="M 276 512 C 356 544 416 480 512 436 C 608 392 660 362 748 402" 
          fill="none" stroke="url(#st-stream-core)" stroke-width="32" stroke-linecap="round"/>
    <circle cx="276" cy="512" r="9" fill="#00F0FF"/>
    <circle cx="276" cy="512" r="18" fill="none" stroke="#00F0FF" stroke-width="3.5" stroke-opacity="0.5"/>
    <circle cx="748" cy="402" r="9" fill="#D946EF"/>
    <circle cx="748" cy="402" r="18" fill="none" stroke="#D946EF" stroke-width="3.5" stroke-opacity="0.5"/>
  </g>

  <!-- Hero Cursor -->
  <g id="st-cursor" filter="url(#st-shadow-cursor)" transform="translate(496, 390) rotate(-10)">
    <path d="M 0 0 L 0 156 L 38 122 L 78 202 L 116 184 L 78 106 L 134 106 Z" 
          fill="#050811" stroke="#050811" stroke-width="15" stroke-linejoin="round" stroke-linecap="round"/>
    <path d="M 0 0 L 0 156 L 38 122 L 78 202 L 116 184 L 78 106 L 134 106 Z" 
          fill="#FFFFFF"/>
    <path d="M 0 0 L 0 126 L 28 102 L 0 0 Z" 
          fill="#F1F5F9" fill-opacity="0.85"/>
  </g>
</svg>
'''

# -----------------------------------------------------------------------------
# 3. Monochrome Light (White / High Contrast Dark Backgrounds)
# -----------------------------------------------------------------------------
SVG_MONO_LIGHT = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <!-- Left Screen -->
  <rect x="196" y="356" width="374" height="268" rx="28" fill="none" stroke="#FFFFFF" stroke-width="16"/>
  <!-- Right Screen (Solid backplate mask to occlude left screen overlap) -->
  <rect x="454" y="292" width="374" height="268" rx="28" fill="#000000" stroke="#FFFFFF" stroke-width="16"/>
  <!-- Flowing Connection Line -->
  <path d="M 276 512 C 356 544 416 480 512 436 C 608 392 660 362 748 402" 
        fill="none" stroke="#FFFFFF" stroke-width="28" stroke-linecap="round"/>
  <circle cx="276" cy="512" r="12" fill="#FFFFFF"/>
  <circle cx="748" cy="402" r="12" fill="#FFFFFF"/>
  <!-- Cursor with dark cutout boundary -->
  <g transform="translate(496, 390) rotate(-10)">
    <path d="M 0 0 L 0 156 L 38 122 L 78 202 L 116 184 L 78 106 L 134 106 Z" 
          fill="#000000" stroke="#000000" stroke-width="24" stroke-linejoin="round" stroke-linecap="round"/>
    <path d="M 0 0 L 0 156 L 38 122 L 78 202 L 116 184 L 78 106 L 134 106 Z" 
          fill="#FFFFFF"/>
  </g>
</svg>
'''

# -----------------------------------------------------------------------------
# 4. Monochrome Dark (Black / High Contrast Light Backgrounds)
# -----------------------------------------------------------------------------
SVG_MONO_DARK = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <!-- Left Screen -->
  <rect x="196" y="356" width="374" height="268" rx="28" fill="none" stroke="#0F172A" stroke-width="16"/>
  <!-- Right Screen -->
  <rect x="454" y="292" width="374" height="268" rx="28" fill="#FFFFFF" stroke="#0F172A" stroke-width="16"/>
  <!-- Flowing Connection Line -->
  <path d="M 276 512 C 356 544 416 480 512 436 C 608 392 660 362 748 402" 
        fill="none" stroke="#0F172A" stroke-width="28" stroke-linecap="round"/>
  <circle cx="276" cy="512" r="12" fill="#0F172A"/>
  <circle cx="748" cy="402" r="12" fill="#0F172A"/>
  <!-- Cursor with light cutout boundary -->
  <g transform="translate(496, 390) rotate(-10)">
    <path d="M 0 0 L 0 156 L 38 122 L 78 202 L 116 184 L 78 106 L 134 106 Z" 
          fill="#FFFFFF" stroke="#FFFFFF" stroke-width="24" stroke-linejoin="round" stroke-linecap="round"/>
    <path d="M 0 0 L 0 156 L 38 122 L 78 202 L 116 184 L 78 106 L 134 106 Z" 
          fill="#0F172A"/>
  </g>
</svg>
'''

# Save all SVGs into assets/branding
with open(f"{BRANDING_DIR}/mouse-share-app-icon.svg", "w", encoding="utf-8") as f:
    f.write(SVG_APP_ICON.strip())

with open(f"{BRANDING_DIR}/mouse-share-standalone.svg", "w", encoding="utf-8") as f:
    f.write(SVG_STANDALONE.strip())

with open(f"{BRANDING_DIR}/mouse-share-mono-light.svg", "w", encoding="utf-8") as f:
    f.write(SVG_MONO_LIGHT.strip())

with open(f"{BRANDING_DIR}/mouse-share-mono-dark.svg", "w", encoding="utf-8") as f:
    f.write(SVG_MONO_DARK.strip())

# Also update the primary app-icon.svg in src-tauri
with open("src-tauri/app-icon.svg", "w", encoding="utf-8") as f:
    f.write(SVG_APP_ICON.strip())

print("All branding SVG assets written successfully!")

# Run tauri icon generation to update src-tauri/icons with official multi-resolution assets
print("Compiling Windows ICO and PNG asset suites via tauri icon...")
result = subprocess.run(["npx", "tauri", "icon", "src-tauri/app-icon.svg", "-o", "src-tauri/icons"], shell=True, capture_output=True, text=True)
print(result.stdout)
if result.stderr:
    print("Stderr:", result.stderr)

# Also copy primary icon.png to website/assets/icon.png if directory exists
if os.path.exists("website/assets"):
    import shutil
    shutil.copy("src-tauri/icons/icon.png", "website/assets/icon.png")
    print("Updated website/assets/icon.png")
