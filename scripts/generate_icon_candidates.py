import os

os.makedirs("dist/candidates", exist_ok=True)

# -------------------------------------------------------------------------
# Candidate 1: "Fluent Flow" - Tiered floating display slates, luminous S-curve stream, crisp white cursor
# -------------------------------------------------------------------------
svg_1 = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <linearGradient id="c1-bg" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0E1626"/>
      <stop offset="50%" stop-color="#0A101C"/>
      <stop offset="100%" stop-color="#060912"/>
    </linearGradient>

    <linearGradient id="c1-tile-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#38BDF8" stop-opacity="0.4"/>
      <stop offset="35%" stop-color="#818CF8" stop-opacity="0.2"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.08"/>
    </linearGradient>

    <radialGradient id="c1-ambient" cx="50%" cy="46%" r="50%">
      <stop offset="0%" stop-color="#0284C7" stop-opacity="0.22"/>
      <stop offset="55%" stop-color="#4F46E5" stop-opacity="0.10"/>
      <stop offset="100%" stop-color="#0284C7" stop-opacity="0"/>
    </radialGradient>

    <!-- Screen 1 (Left) -->
    <linearGradient id="c1-s1-bezel" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#1F2A3D"/>
      <stop offset="100%" stop-color="#101827"/>
    </linearGradient>
    <linearGradient id="c1-s1-disp" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0E2347"/>
      <stop offset="60%" stop-color="#0B1833"/>
      <stop offset="100%" stop-color="#081022"/>
    </linearGradient>
    <linearGradient id="c1-s1-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#60A5FA" stop-opacity="0.35"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.1"/>
    </linearGradient>

    <!-- Screen 2 (Right) -->
    <linearGradient id="c1-s2-bezel" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#24344E"/>
      <stop offset="100%" stop-color="#121D30"/>
    </linearGradient>
    <linearGradient id="c1-s2-disp" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#12366E"/>
      <stop offset="50%" stop-color="#0F244E"/>
      <stop offset="100%" stop-color="#0A1633"/>
    </linearGradient>
    <linearGradient id="c1-s2-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#38BDF8" stop-opacity="0.6"/>
      <stop offset="60%" stop-color="#818CF8" stop-opacity="0.3"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.15"/>
    </linearGradient>

    <!-- Stream Gradient -->
    <linearGradient id="c1-stream" x1="0%" y1="60%" x2="100%" y2="40%">
      <stop offset="0%" stop-color="#00D8F6"/>
      <stop offset="25%" stop-color="#38BDF8"/>
      <stop offset="55%" stop-color="#3B82F6"/>
      <stop offset="80%" stop-color="#6366F1"/>
      <stop offset="100%" stop-color="#A855F7"/>
    </linearGradient>

    <filter id="c1-glow" x="-20%" y="-40%" width="140%" height="180%">
      <feGaussianBlur stdDeviation="10" result="blur"/>
      <feComposite in="SourceGraphic" in2="blur" operator="over"/>
    </filter>

    <filter id="c1-shadow" x="-30%" y="-30%" width="160%" height="170%">
      <feDropShadow dx="-14" dy="16" stdDeviation="22" flood-color="#000000" flood-opacity="0.6"/>
    </filter>

    <filter id="c1-cursor-shadow" x="-30%" y="-30%" width="160%" height="160%">
      <feDropShadow dx="-6" dy="10" stdDeviation="12" flood-color="#000000" flood-opacity="0.75"/>
    </filter>
  </defs>

  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="url(#c1-bg)"/>
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="url(#c1-ambient)"/>
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="none" stroke="url(#c1-tile-rim)" stroke-width="2.5"/>

  <!-- Left Screen -->
  <g id="c1-screen-left">
    <rect x="196" y="360" width="376" height="268" rx="28" fill="url(#c1-s1-bezel)" stroke="url(#c1-s1-rim)" stroke-width="2"/>
    <rect x="210" y="374" width="348" height="240" rx="18" fill="url(#c1-s1-disp)"/>
    <line x1="210" y1="586" x2="558" y2="586" stroke="#38BDF8" stroke-opacity="0.12" stroke-width="1.5"/>
    <circle cx="236" cy="396" r="4" fill="#38BDF8" fill-opacity="0.3"/>
    <circle cx="252" cy="396" r="4" fill="#818CF8" fill-opacity="0.3"/>
  </g>

  <!-- Right Screen -->
  <g id="c1-screen-right" filter="url(#c1-shadow)">
    <rect x="452" y="296" width="376" height="268" rx="28" fill="url(#c1-s2-bezel)" stroke="url(#c1-s2-rim)" stroke-width="2.5"/>
    <rect x="466" y="310" width="348" height="240" rx="18" fill="url(#c1-s2-disp)"/>
    <line x1="466" y1="522" x2="814" y2="522" stroke="#60A5FA" stroke-opacity="0.15" stroke-width="1.5"/>
    <circle cx="492" cy="332" r="4" fill="#38BDF8" fill-opacity="0.4"/>
    <circle cx="508" cy="332" r="4" fill="#A855F7" fill-opacity="0.35"/>
  </g>

  <!-- Flowing Bridge -->
  <g id="c1-bridge">
    <path d="M 284 508 C 364 538 418 474 512 434 C 606 394 660 366 740 404" 
          fill="none" stroke="url(#c1-stream)" stroke-width="44" stroke-linecap="round" opacity="0.32" filter="url(#c1-glow)"/>
    <path d="M 284 508 C 364 538 418 474 512 434 C 606 394 660 366 740 404" 
          fill="none" stroke="url(#c1-stream)" stroke-width="26" stroke-linecap="round"/>
    
    <circle cx="284" cy="508" r="9" fill="#00D8F6"/>
    <circle cx="284" cy="508" r="17" fill="none" stroke="#00D8F6" stroke-width="3" stroke-opacity="0.45"/>

    <circle cx="740" cy="404" r="9" fill="#A855F7"/>
    <circle cx="740" cy="404" r="17" fill="none" stroke="#A855F7" stroke-width="3" stroke-opacity="0.45"/>
  </g>

  <!-- Cursor -->
  <g id="c1-cursor" filter="url(#c1-cursor-shadow)" transform="translate(488, 386)">
    <path d="M 0 0 L 0 148 L 38 116 L 76 194 L 112 176 L 74 100 L 128 100 Z" 
          fill="#070C16" stroke="#070C16" stroke-width="14" stroke-linejoin="round" stroke-linecap="round"/>
    <path d="M 0 0 L 0 148 L 38 116 L 76 194 L 112 176 L 74 100 L 128 100 Z" 
          fill="#FFFFFF"/>
    <path d="M 0 0 L 0 120 L 26 98 L 4 0 Z" 
          fill="#F8FAFC" fill-opacity="0.9"/>
  </g>
</svg>
'''

# -------------------------------------------------------------------------
# Candidate 2: "Unified Horizon" - Dual desktop monitors with sleek architectural stands, dynamic infinite wave conduit, forward-tilted cursor
# -------------------------------------------------------------------------
svg_2 = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <linearGradient id="c2-bg" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0D1524"/>
      <stop offset="45%" stop-color="#090E1A"/>
      <stop offset="100%" stop-color="#04060D"/>
    </linearGradient>

    <linearGradient id="c2-tile-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#00D8F6" stop-opacity="0.45"/>
      <stop offset="40%" stop-color="#6366F1" stop-opacity="0.25"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.05"/>
    </linearGradient>

    <radialGradient id="c2-ambient" cx="50%" cy="48%" r="48%">
      <stop offset="0%" stop-color="#0284C7" stop-opacity="0.24"/>
      <stop offset="50%" stop-color="#4F46E5" stop-opacity="0.12"/>
      <stop offset="100%" stop-color="#0284C7" stop-opacity="0"/>
    </radialGradient>

    <!-- Monitor Gradients -->
    <linearGradient id="c2-stand" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#1E293B"/>
      <stop offset="50%" stop-color="#334155"/>
      <stop offset="100%" stop-color="#1E293B"/>
    </linearGradient>

    <linearGradient id="c2-m1-bezel" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#1E2A3C"/>
      <stop offset="100%" stop-color="#0D1422"/>
    </linearGradient>
    <linearGradient id="c2-m1-disp" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0F2852"/>
      <stop offset="70%" stop-color="#091836"/>
      <stop offset="100%" stop-color="#060F22"/>
    </linearGradient>

    <linearGradient id="c2-m2-bezel" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#263750"/>
      <stop offset="100%" stop-color="#121C2E"/>
    </linearGradient>
    <linearGradient id="c2-m2-disp" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#163C78"/>
      <stop offset="60%" stop-color="#102552"/>
      <stop offset="100%" stop-color="#081430"/>
    </linearGradient>

    <!-- Stream Gradient -->
    <linearGradient id="c2-stream" x1="0%" y1="50%" x2="100%" y2="50%">
      <stop offset="0%" stop-color="#00F0FF"/>
      <stop offset="30%" stop-color="#00A2FF"/>
      <stop offset="70%" stop-color="#6E56CF"/>
      <stop offset="100%" stop-color="#BF40BF"/>
    </linearGradient>

    <filter id="c2-shadow" x="-30%" y="-30%" width="160%" height="170%">
      <feDropShadow dx="-12" dy="18" stdDeviation="22" flood-color="#000000" flood-opacity="0.65"/>
    </filter>

    <filter id="c2-cursor-shadow" x="-30%" y="-30%" width="160%" height="160%">
      <feDropShadow dx="-4" dy="12" stdDeviation="12" flood-color="#000000" flood-opacity="0.8"/>
    </filter>
  </defs>

  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="url(#c2-bg)"/>
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="url(#c2-ambient)"/>
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="none" stroke="url(#c2-tile-rim)" stroke-width="2.5"/>

  <!-- Left Monitor Base/Stand -->
  <g id="c2-m1-stand">
    <rect x="368" y="608" width="36" height="36" rx="4" fill="url(#c2-stand)"/>
    <rect x="316" y="638" width="140" height="12" rx="6" fill="#2A384F"/>
    <rect x="316" y="638" width="140" height="12" rx="6" fill="none" stroke="#475569" stroke-width="1.5" stroke-opacity="0.4"/>
  </g>

  <!-- Left Monitor Screen -->
  <g id="c2-m1">
    <rect x="196" y="348" width="376" height="268" rx="26" fill="url(#c2-m1-bezel)" stroke="#38BDF8" stroke-width="1.5" stroke-opacity="0.3"/>
    <rect x="210" y="362" width="348" height="240" rx="18" fill="url(#c2-m1-disp)"/>
    <!-- Subtle screen wallpaper glow -->
    <path d="M 210 520 Q 300 480 400 540 T 558 500 L 558 602 L 210 602 Z" fill="#0078D4" fill-opacity="0.12"/>
  </g>

  <!-- Right Monitor Stand -->
  <g id="c2-m2-stand" filter="url(#c2-shadow)">
    <rect x="622" y="556" width="36" height="36" rx="4" fill="url(#c2-stand)"/>
    <rect x="570" y="586" width="140" height="12" rx="6" fill="#33435C"/>
    <rect x="570" y="586" width="140" height="12" rx="6" fill="none" stroke="#60A5FA" stroke-width="1.5" stroke-opacity="0.4"/>
  </g>

  <!-- Right Monitor Screen (Elevated & Overlapping) -->
  <g id="c2-m2" filter="url(#c2-shadow)">
    <rect x="452" y="296" width="376" height="268" rx="26" fill="url(#c2-m2-bezel)" stroke="#38BDF8" stroke-width="2" stroke-opacity="0.5"/>
    <rect x="466" y="310" width="348" height="240" rx="18" fill="url(#c2-m2-disp)"/>
    <path d="M 466 468 Q 556 428 656 488 T 814 448 L 814 550 L 466 550 Z" fill="#38BDF8" fill-opacity="0.14"/>
  </g>

  <!-- The Elegant Continuous Flowing Line -->
  <g id="c2-flow">
    <!-- Glow -->
    <path d="M 284 492 C 374 528 424 456 512 424 C 600 392 654 364 740 398" 
          fill="none" stroke="url(#c2-stream)" stroke-width="40" stroke-linecap="round" opacity="0.35"/>
    <!-- Core line -->
    <path d="M 284 492 C 374 528 424 456 512 424 C 600 392 654 364 740 398" 
          fill="none" stroke="url(#c2-stream)" stroke-width="26" stroke-linecap="round"/>

    <!-- Left Origin Orb -->
    <circle cx="284" cy="492" r="10" fill="#00F0FF"/>
    <circle cx="284" cy="492" r="18" fill="none" stroke="#00F0FF" stroke-width="3" stroke-opacity="0.5"/>

    <!-- Right Target Orb -->
    <circle cx="740" cy="398" r="10" fill="#BF40BF"/>
    <circle cx="740" cy="398" r="18" fill="none" stroke="#BF40BF" stroke-width="3" stroke-opacity="0.5"/>
  </g>

  <!-- Tilted Dynamic Cursor at Crossing -->
  <g id="c2-cursor" filter="url(#c2-cursor-shadow)" transform="translate(516, 420) rotate(-14)">
    <path d="M 0 0 L 0 148 L 38 116 L 76 194 L 112 176 L 74 100 L 128 100 Z" 
          fill="#070C16" stroke="#070C16" stroke-width="14" stroke-linejoin="round" stroke-linecap="round"/>
    <path d="M 0 0 L 0 148 L 38 116 L 76 194 L 112 176 L 74 100 L 128 100 Z" 
          fill="#FFFFFF"/>
    <path d="M 0 0 L 0 120 L 26 98 L 4 0 Z" 
          fill="#F1F5F9" fill-opacity="0.85"/>
  </g>
</svg>
'''

# -------------------------------------------------------------------------
# Candidate 3: "Precision Workstation" - Ultimate Fluent 2.5D displays, dual desktop panels, luminous laser conduit, iconic white cursor with subtle glass sheen
# -------------------------------------------------------------------------
svg_3 = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <!-- Background Squircle Gradient -->
    <linearGradient id="c3-bg" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0B1322"/>
      <stop offset="50%" stop-color="#080D18"/>
      <stop offset="100%" stop-color="#04060B"/>
    </linearGradient>

    <!-- Fluent Glass Rim for App Tile -->
    <linearGradient id="c3-tile-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#38BDF8" stop-opacity="0.45"/>
      <stop offset="35%" stop-color="#818CF8" stop-opacity="0.25"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.08"/>
    </linearGradient>

    <!-- Radial Ambient Backlight -->
    <radialGradient id="c3-ambient" cx="50%" cy="46%" r="52%">
      <stop offset="0%" stop-color="#0284C7" stop-opacity="0.28"/>
      <stop offset="45%" stop-color="#4F46E5" stop-opacity="0.14"/>
      <stop offset="100%" stop-color="#0B1322" stop-opacity="0"/>
    </radialGradient>

    <!-- Left Screen Shading -->
    <linearGradient id="c3-s1-frame" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#1E293B"/>
      <stop offset="100%" stop-color="#0F172A"/>
    </linearGradient>
    <linearGradient id="c3-s1-glass" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0C2042"/>
      <stop offset="50%" stop-color="#08142C"/>
      <stop offset="100%" stop-color="#050C1C"/>
    </linearGradient>

    <!-- Right Screen Shading (Higher elevation) -->
    <linearGradient id="c3-s2-frame" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#283953"/>
      <stop offset="100%" stop-color="#131D2E"/>
    </linearGradient>
    <linearGradient id="c3-s2-glass" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#113369"/>
      <stop offset="50%" stop-color="#0D2247"/>
      <stop offset="100%" stop-color="#071329"/>
    </linearGradient>

    <!-- Screen Bezel Specular Rims -->
    <linearGradient id="c3-s1-specular" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#60A5FA" stop-opacity="0.4"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.1"/>
    </linearGradient>
    <linearGradient id="c3-s2-specular" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#38BDF8" stop-opacity="0.7"/>
      <stop offset="50%" stop-color="#818CF8" stop-opacity="0.4"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.15"/>
    </linearGradient>

    <!-- Flowing Line Gradient (Cyan -> Electric Blue -> Deep Violet) -->
    <linearGradient id="c3-flow-grad" x1="0%" y1="60%" x2="100%" y2="40%">
      <stop offset="0%" stop-color="#00E5FF"/>
      <stop offset="25%" stop-color="#0284C7"/>
      <stop offset="55%" stop-color="#3B82F6"/>
      <stop offset="80%" stop-color="#6366F1"/>
      <stop offset="100%" stop-color="#A855F7"/>
    </linearGradient>

    <!-- Shadows -->
    <filter id="c3-drop-shadow" x="-30%" y="-30%" width="160%" height="170%">
      <feDropShadow dx="-16" dy="20" stdDeviation="26" flood-color="#000000" flood-opacity="0.7"/>
    </filter>
    <filter id="c3-cursor-shadow" x="-30%" y="-30%" width="160%" height="160%">
      <feDropShadow dx="-6" dy="12" stdDeviation="14" flood-color="#000000" flood-opacity="0.8"/>
    </filter>
  </defs>

  <!-- App Squircle Canvas -->
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="url(#c3-bg)"/>
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="url(#c3-ambient)"/>
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="none" stroke="url(#c3-tile-rim)" stroke-width="2.5"/>

  <!-- Left Screen (Primary Workstation) -->
  <g id="c3-screen-left">
    <!-- Outer Frame -->
    <rect x="196" y="360" width="376" height="268" rx="28" fill="url(#c3-s1-frame)" stroke="url(#c3-s1-specular)" stroke-width="2"/>
    <!-- Active Display Surface -->
    <rect x="210" y="374" width="348" height="240" rx="18" fill="url(#c3-s1-glass)"/>
    <!-- Subtle Desktop Header / Status Dots -->
    <circle cx="236" cy="396" r="4.5" fill="#38BDF8" fill-opacity="0.4"/>
    <circle cx="254" cy="396" r="4.5" fill="#818CF8" fill-opacity="0.3"/>
    <line x1="280" y1="396" x2="350" y2="396" stroke="#38BDF8" stroke-opacity="0.15" stroke-width="2" stroke-linecap="round"/>
    <!-- Subtle Baseline Dock Line -->
    <line x1="210" y1="586" x2="558" y2="586" stroke="#38BDF8" stroke-opacity="0.12" stroke-width="1.5"/>
  </g>

  <!-- Right Screen (Secondary Workstation - Overlapping with High Depth) -->
  <g id="c3-screen-right" filter="url(#c3-drop-shadow)">
    <!-- Outer Frame -->
    <rect x="452" y="296" width="376" height="268" rx="28" fill="url(#c3-s2-frame)" stroke="url(#c3-s2-specular)" stroke-width="2.5"/>
    <!-- Active Display Surface -->
    <rect x="466" y="310" width="348" height="240" rx="18" fill="url(#c3-s2-glass)"/>
    <!-- Subtle Desktop Header / Status Dots -->
    <circle cx="492" cy="332" r="4.5" fill="#00E5FF" fill-opacity="0.5"/>
    <circle cx="510" cy="332" r="4.5" fill="#A855F7" fill-opacity="0.4"/>
    <line x1="536" y1="332" x2="606" y2="332" stroke="#60A5FA" stroke-opacity="0.18" stroke-width="2" stroke-linecap="round"/>
    <!-- Subtle Baseline Dock Line -->
    <line x1="466" y1="522" x2="814" y2="522" stroke="#60A5FA" stroke-opacity="0.16" stroke-width="1.5"/>
  </g>

  <!-- Flowing Connection Conduit -->
  <g id="c3-flow">
    <!-- Ambient Energy Aura -->
    <path d="M 284 508 C 364 542 416 476 512 434 C 608 392 660 364 740 404" 
          fill="none" stroke="url(#c3-flow-grad)" stroke-width="48" stroke-linecap="round" opacity="0.30"/>
    <!-- Luminous Core Wave -->
    <path d="M 284 508 C 364 542 416 476 512 434 C 608 392 660 364 740 404" 
          fill="none" stroke="url(#c3-flow-grad)" stroke-width="28" stroke-linecap="round"/>

    <!-- Left Connection Node -->
    <circle cx="284" cy="508" r="9" fill="#00E5FF"/>
    <circle cx="284" cy="508" r="18" fill="none" stroke="#00E5FF" stroke-width="3" stroke-opacity="0.4"/>

    <!-- Right Connection Node -->
    <circle cx="740" cy="404" r="9" fill="#A855F7"/>
    <circle cx="740" cy="404" r="18" fill="none" stroke="#A855F7" stroke-width="3" stroke-opacity="0.4"/>
  </g>

  <!-- Hero Mouse Cursor (Sharp, Iconic, High-Contrast OS Pointer) -->
  <g id="c3-cursor" filter="url(#c3-cursor-shadow)" transform="translate(488, 386)">
    <!-- High-contrast dark outer stroke ensuring 100% legibility on light/dark/vibrant backgrounds -->
    <path d="M 0 0 L 0 152 L 38 118 L 78 198 L 114 180 L 76 102 L 132 102 Z" 
          fill="#060911" stroke="#060911" stroke-width="14" stroke-linejoin="round" stroke-linecap="round"/>
    <!-- Pure White Hero Fill -->
    <path d="M 0 0 L 0 152 L 38 118 L 78 198 L 114 180 L 76 102 L 132 102 Z" 
          fill="#FFFFFF"/>
    <!-- Subtle directional facet for Fluent glass depth -->
    <path d="M 0 0 L 0 124 L 28 100 L 0 0 Z" 
          fill="#F1F5F9" fill-opacity="0.8"/>
  </g>
</svg>
'''

with open("dist/candidates/candidate-1.svg", "w", encoding="utf-8") as f:
    f.write(svg_1.strip())

with open("dist/candidates/candidate-2.svg", "w", encoding="utf-8") as f:
    f.write(svg_2.strip())

with open("dist/candidates/candidate-3.svg", "w", encoding="utf-8") as f:
    f.write(svg_3.strip())

print("Candidates 1, 2, 3 generated successfully!")
