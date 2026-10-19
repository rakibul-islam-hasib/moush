import os

svg_4 = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <!-- Background Midnight Navy Gradient -->
    <linearGradient id="m-bg" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0C1424"/>
      <stop offset="45%" stop-color="#080E1A"/>
      <stop offset="100%" stop-color="#04060C"/>
    </linearGradient>

    <!-- Acrylic Squircle Border Highlight -->
    <linearGradient id="m-tile-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#38BDF8" stop-opacity="0.45"/>
      <stop offset="30%" stop-color="#818CF8" stop-opacity="0.25"/>
      <stop offset="70%" stop-color="#1E293B" stop-opacity="0.10"/>
      <stop offset="100%" stop-color="#0F172A" stop-opacity="0.05"/>
    </linearGradient>

    <!-- Radial Ambient Backlight -->
    <radialGradient id="m-ambient" cx="50%" cy="46%" r="52%">
      <stop offset="0%" stop-color="#0284C7" stop-opacity="0.26"/>
      <stop offset="45%" stop-color="#4F46E5" stop-opacity="0.14"/>
      <stop offset="85%" stop-color="#04060C" stop-opacity="0"/>
    </radialGradient>

    <!-- Left Screen (Machine 1 - Primary Source) -->
    <linearGradient id="m-s1-frame" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#1C273A"/>
      <stop offset="100%" stop-color="#0E1626"/>
    </linearGradient>
    <linearGradient id="m-s1-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#60A5FA" stop-opacity="0.4"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.1"/>
    </linearGradient>
    <linearGradient id="m-s1-glass" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0D2246"/>
      <stop offset="60%" stop-color="#08152E"/>
      <stop offset="100%" stop-color="#050E20"/>
    </linearGradient>

    <!-- Right Screen (Machine 2 - Receiving Target, Higher Depth) -->
    <linearGradient id="m-s2-frame" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#253650"/>
      <stop offset="100%" stop-color="#121D30"/>
    </linearGradient>
    <linearGradient id="m-s2-rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#38BDF8" stop-opacity="0.75"/>
      <stop offset="50%" stop-color="#818CF8" stop-opacity="0.45"/>
      <stop offset="100%" stop-color="#1E293B" stop-opacity="0.15"/>
    </linearGradient>
    <linearGradient id="m-s2-glass" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#12356B"/>
      <stop offset="50%" stop-color="#0E234A"/>
      <stop offset="100%" stop-color="#07142E"/>
    </linearGradient>

    <!-- Flowing Stream Core Gradient (Neon Cyan -> Blue -> Indigo -> Violet -> Magenta) -->
    <linearGradient id="m-stream-core" x1="0%" y1="60%" x2="100%" y2="40%">
      <stop offset="0%" stop-color="#00F0FF"/>
      <stop offset="25%" stop-color="#0284C7"/>
      <stop offset="50%" stop-color="#3B82F6"/>
      <stop offset="75%" stop-color="#6366F1"/>
      <stop offset="90%" stop-color="#A855F7"/>
      <stop offset="100%" stop-color="#D946EF"/>
    </linearGradient>

    <!-- Flowing Stream Ambient Glow Gradient -->
    <linearGradient id="m-stream-glow" x1="0%" y1="60%" x2="100%" y2="40%">
      <stop offset="0%" stop-color="#00E5FF"/>
      <stop offset="50%" stop-color="#38BDF8"/>
      <stop offset="100%" stop-color="#A855F7"/>
    </linearGradient>

    <!-- Fluent Elevation Shadows -->
    <filter id="m-shadow-s2" x="-35%" y="-35%" width="170%" height="180%">
      <feDropShadow dx="-16" dy="20" stdDeviation="24" flood-color="#000000" flood-opacity="0.72"/>
    </filter>

    <filter id="m-shadow-cursor" x="-35%" y="-35%" width="170%" height="170%">
      <feDropShadow dx="-4" dy="12" stdDeviation="14" flood-color="#000000" flood-opacity="0.82"/>
    </filter>

    <filter id="m-blur-glow" x="-25%" y="-50%" width="150%" height="200%">
      <feGaussianBlur stdDeviation="12" result="blur"/>
      <feComposite in="SourceGraphic" in2="blur" operator="over"/>
    </filter>
  </defs>

  <!-- 1. Master Windows 11 Fluent Squircle Base (864 × 864, Safe Margins 80px) -->
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="url(#m-bg)"/>
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="url(#m-ambient)"/>
  <rect x="80" y="80" width="864" height="864" rx="196" ry="196" fill="none" stroke="url(#m-tile-rim)" stroke-width="2.5"/>

  <!-- 2. Screen 1 (Left - Workstation A) -->
  <g id="screen-left">
    <!-- Outer Bezel -->
    <rect x="196" y="356" width="374" height="268" rx="28" fill="url(#m-s1-frame)" stroke="url(#m-s1-rim)" stroke-width="2"/>
    <!-- Active Display Surface -->
    <rect x="210" y="370" width="346" height="240" rx="18" fill="url(#m-s1-glass)"/>
    
    <!-- Screen 1 UI / Workspace Affordances -->
    <circle cx="236" cy="392" r="4.5" fill="#38BDF8" fill-opacity="0.4"/>
    <circle cx="254" cy="392" r="4.5" fill="#818CF8" fill-opacity="0.3"/>
    <line x1="278" y1="392" x2="344" y2="392" stroke="#38BDF8" stroke-opacity="0.15" stroke-width="2" stroke-linecap="round"/>
    
    <!-- Workspace Desktop Card 1 -->
    <rect x="240" y="420" width="136" height="92" rx="10" fill="#1E3A8A" fill-opacity="0.18" stroke="#38BDF8" stroke-opacity="0.16" stroke-width="1.5"/>
    <line x1="256" y1="442" x2="320" y2="442" stroke="#60A5FA" stroke-opacity="0.25" stroke-width="2" stroke-linecap="round"/>
    <line x1="256" y1="458" x2="296" y2="458" stroke="#60A5FA" stroke-opacity="0.18" stroke-width="2" stroke-linecap="round"/>

    <!-- Bottom Dock / Taskbar Accent -->
    <line x1="210" y1="584" x2="556" y2="584" stroke="#38BDF8" stroke-opacity="0.12" stroke-width="1.5"/>
  </g>

  <!-- 3. Screen 2 (Right - Workstation B, Elevated with Ambient Shadow) -->
  <g id="screen-right" filter="url(#m-shadow-s2)">
    <!-- Outer Bezel -->
    <rect x="454" y="292" width="374" height="268" rx="28" fill="url(#m-s2-frame)" stroke="url(#m-s2-rim)" stroke-width="2.5"/>
    <!-- Active Display Surface -->
    <rect x="468" y="306" width="346" height="240" rx="18" fill="url(#m-s2-glass)"/>

    <!-- Screen 2 UI / Workspace Affordances -->
    <circle cx="494" cy="328" r="4.5" fill="#00F0FF" fill-opacity="0.55"/>
    <circle cx="512" cy="328" r="4.5" fill="#A855F7" fill-opacity="0.45"/>
    <line x1="536" y1="328" x2="602" y2="328" stroke="#60A5FA" stroke-opacity="0.22" stroke-width="2" stroke-linecap="round"/>

    <!-- Workspace Desktop Card 2 -->
    <rect x="646" y="348" width="136" height="92" rx="10" fill="#0284C7" fill-opacity="0.22" stroke="#38BDF8" stroke-opacity="0.3" stroke-width="1.5"/>
    <line x1="662" y1="370" x2="726" y2="370" stroke="#38BDF8" stroke-opacity="0.35" stroke-width="2" stroke-linecap="round"/>
    <line x1="662" y1="386" x2="702" y2="386" stroke="#38BDF8" stroke-opacity="0.25" stroke-width="2" stroke-linecap="round"/>

    <!-- Bottom Dock / Taskbar Accent -->
    <line x1="468" y1="520" x2="814" y2="520" stroke="#60A5FA" stroke-opacity="0.16" stroke-width="1.5"/>
  </g>

  <!-- 4. The Flowing Connection Stream (Seamless Conduit) -->
  <g id="flowing-conduit">
    <!-- Ambient Luminous Glow Path -->
    <path d="M 276 512 C 356 544 416 480 512 436 C 608 392 660 362 748 402" 
          fill="none" stroke="url(#m-stream-glow)" stroke-width="46" stroke-linecap="round" opacity="0.28" filter="url(#m-blur-glow)"/>
    
    <!-- Sharp Core Energy Stream -->
    <path d="M 276 512 C 356 544 416 480 512 436 C 608 392 660 362 748 402" 
          fill="none" stroke="url(#m-stream-core)" stroke-width="28" stroke-linecap="round"/>

    <!-- Source Connection Terminal (Screen 1) -->
    <circle cx="276" cy="512" r="8.5" fill="#00F0FF"/>
    <circle cx="276" cy="512" r="17" fill="none" stroke="#00F0FF" stroke-width="3" stroke-opacity="0.45"/>

    <!-- Target Connection Terminal (Screen 2) -->
    <circle cx="748" cy="402" r="8.5" fill="#D946EF"/>
    <circle cx="748" cy="402" r="17" fill="none" stroke="#D946EF" stroke-width="3" stroke-opacity="0.45"/>
  </g>

  <!-- 5. The Hero Mouse Cursor (Sharp, Crisp, Precision-Tilted Pointer) -->
  <!-- Positioned at the pinnacle crossing point (X=496, Y=390), tilted -10° along the flow line -->
  <g id="mouse-cursor-hero" filter="url(#m-shadow-cursor)" transform="translate(496, 390) rotate(-10)">
    <!-- High-Contrast Dark Protective Stroke (Ensures instant silhouette isolation on any background) -->
    <path d="M 0 0 L 0 152 L 38 118 L 78 198 L 114 180 L 76 102 L 132 102 Z" 
          fill="#050811" stroke="#050811" stroke-width="14" stroke-linejoin="round" stroke-linecap="round"/>
    
    <!-- Pure White Hero Fill -->
    <path d="M 0 0 L 0 152 L 38 118 L 78 198 L 114 180 L 76 102 L 132 102 Z" 
          fill="#FFFFFF"/>
    
    <!-- Subtle Fluent Bevel / Sheen for Visual Richness -->
    <path d="M 0 0 L 0 124 L 28 100 L 0 0 Z" 
          fill="#F1F5F9" fill-opacity="0.82"/>
  </g>
</svg>
'''

with open("dist/candidates/candidate-4.svg", "w", encoding="utf-8") as f:
    f.write(svg_4.strip())

print("Candidate 4 created successfully!")
