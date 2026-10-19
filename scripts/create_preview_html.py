html_content = '''<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Mouse Share - App Icon Review & Comparison</title>
  <style>
    :root {
      --bg: #0B0F17;
      --card: #131A26;
      --border: #222F44;
      --text: #F1F5F9;
      --muted: #94A3B8;
      --accent: #38BDF8;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body {
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
      background: var(--bg);
      color: var(--text);
      padding: 40px 24px;
      line-height: 1.5;
    }
    .header {
      max-width: 1200px;
      margin: 0 auto 36px auto;
      text-align: center;
    }
    h1 { font-size: 28px; font-weight: 700; margin-bottom: 8px; color: #FFF; }
    .subtitle { font-size: 15px; color: var(--muted); }
    
    .grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));
      gap: 28px;
      max-width: 1300px;
      margin: 0 auto 48px auto;
    }
    .candidate-card {
      background: var(--card);
      border: 1px solid var(--border);
      border-radius: 16px;
      padding: 24px;
      display: flex;
      flex-direction: column;
      gap: 20px;
    }
    .card-title {
      font-size: 18px;
      font-weight: 600;
      color: var(--accent);
      display: flex;
      justify-content: space-between;
      align-items: center;
    }
    .tag {
      font-size: 11px;
      text-transform: uppercase;
      letter-spacing: 0.05em;
      padding: 4px 10px;
      border-radius: 20px;
      background: rgba(56, 189, 248, 0.15);
      color: #38BDF8;
    }
    .preview-box {
      background: #060911;
      border: 1px solid #1E293B;
      border-radius: 12px;
      padding: 24px;
      display: flex;
      align-items: center;
      justify-content: center;
    }
    .preview-box.light {
      background: #E2E8F0;
      border-color: #CBD5E1;
    }
    .icon-img {
      display: block;
      transition: transform 0.2s;
    }
    .size-strip {
      display: flex;
      align-items: flex-end;
      gap: 20px;
      padding: 16px;
      background: #090E17;
      border-radius: 10px;
      border: 1px solid #1A2333;
    }
    .size-item {
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 8px;
    }
    .size-label {
      font-size: 11px;
      color: var(--muted);
    }

    /* Windows 11 Taskbar Simulation */
    .taskbar-sim {
      max-width: 1200px;
      margin: 40px auto;
      background: rgba(20, 24, 33, 0.85);
      backdrop-filter: blur(20px);
      border: 1px solid rgba(255, 255, 255, 0.1);
      border-radius: 12px;
      padding: 12px 24px;
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 16px;
    }
    .taskbar-icon {
      width: 36px;
      height: 36px;
      border-radius: 6px;
      display: flex;
      align-items: center;
      justify-content: center;
      position: relative;
      cursor: pointer;
    }
    .taskbar-icon.active::after {
      content: "";
      position: absolute;
      bottom: -8px;
      width: 16px;
      height: 3px;
      border-radius: 2px;
      background: #38BDF8;
    }
  </style>
</head>
<body>

  <div class="header">
    <h1>Mouse Share &mdash; Application Icon Exploration</h1>
    <p class="subtitle">Windows 11 Fluent Design &bull; Rust-Powered Performance &bull; Multi-Screen Connectivity</p>
  </div>

  <div class="grid">
    <!-- Candidate 1 -->
    <div class="candidate-card">
      <div class="card-title">
        <span>Concept 1: Fluent Flow</span>
        <span class="tag">Tiered Floating</span>
      </div>
      <div class="preview-box">
        <img class="icon-img" src="candidate-1.svg" width="220" height="220" alt="Concept 1">
      </div>
      <div class="size-strip">
        <div class="size-item">
          <img src="candidate-1.svg" width="64" height="64">
          <span class="size-label">64px</span>
        </div>
        <div class="size-item">
          <img src="candidate-1.svg" width="48" height="48">
          <span class="size-label">48px</span>
        </div>
        <div class="size-item">
          <img src="candidate-1.svg" width="32" height="32">
          <span class="size-label">32px</span>
        </div>
        <div class="size-item">
          <img src="candidate-1.svg" width="24" height="24">
          <span class="size-label">24px</span>
        </div>
        <div class="size-item">
          <img src="candidate-1.svg" width="16" height="16">
          <span class="size-label">16px</span>
        </div>
      </div>
      <div class="preview-box light">
        <img class="icon-img" src="candidate-1.svg" width="100" height="100" alt="Concept 1 on Light">
      </div>
    </div>

    <!-- Candidate 2 -->
    <div class="candidate-card">
      <div class="card-title">
        <span>Concept 2: Unified Horizon</span>
        <span class="tag">With Pedestals</span>
      </div>
      <div class="preview-box">
        <img class="icon-img" src="candidate-2.svg" width="220" height="220" alt="Concept 2">
      </div>
      <div class="size-strip">
        <div class="size-item">
          <img src="candidate-2.svg" width="64" height="64">
          <span class="size-label">64px</span>
        </div>
        <div class="size-item">
          <img src="candidate-2.svg" width="48" height="48">
          <span class="size-label">48px</span>
        </div>
        <div class="size-item">
          <img src="candidate-2.svg" width="32" height="32">
          <span class="size-label">32px</span>
        </div>
        <div class="size-item">
          <img src="candidate-2.svg" width="24" height="24">
          <span class="size-label">24px</span>
        </div>
        <div class="size-item">
          <img src="candidate-2.svg" width="16" height="16">
          <span class="size-label">16px</span>
        </div>
      </div>
      <div class="preview-box light">
        <img class="icon-img" src="candidate-2.svg" width="100" height="100" alt="Concept 2 on Light">
      </div>
    </div>

    <!-- Candidate 3 -->
    <div class="candidate-card">
      <div class="card-title">
        <span>Concept 3: Precision Workstation</span>
        <span class="tag">High-Contrast Hero</span>
      </div>
      <div class="preview-box">
        <img class="icon-img" src="candidate-3.svg" width="220" height="220" alt="Concept 3">
      </div>
      <div class="size-strip">
        <div class="size-item">
          <img src="candidate-3.svg" width="64" height="64">
          <span class="size-label">64px</span>
        </div>
        <div class="size-item">
          <img src="candidate-3.svg" width="48" height="48">
          <span class="size-label">48px</span>
        </div>
        <div class="size-item">
          <img src="candidate-3.svg" width="32" height="32">
          <span class="size-label">32px</span>
        </div>
        <div class="size-item">
          <img src="candidate-3.svg" width="24" height="24">
          <span class="size-label">24px</span>
        </div>
        <div class="size-item">
          <img src="candidate-3.svg" width="16" height="16">
          <span class="size-label">16px</span>
        </div>
      </div>
      <div class="preview-box light">
        <img class="icon-img" src="candidate-3.svg" width="100" height="100" alt="Concept 3 on Light">
      </div>
    </div>
  </div>

  <!-- Windows 11 Taskbar Simulation -->
  <div style="text-align: center; margin-bottom: 12px; font-size: 13px; color: var(--muted);">Windows 11 Taskbar Simulation (32px icons)</div>
  <div class="taskbar-sim">
    <div class="taskbar-icon"><img src="candidate-1.svg" width="30" height="30"></div>
    <div class="taskbar-icon"><img src="candidate-2.svg" width="30" height="30"></div>
    <div class="taskbar-icon active"><img src="candidate-3.svg" width="30" height="30"></div>
  </div>

</body>
</html>
'''

with open("dist/candidates/preview.html", "w", encoding="utf-8") as f:
    f.write(html_content)

print("preview.html created successfully!")
