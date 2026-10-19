/**
 * Moush Website — Interactive Experience & Workstation Simulator
 * Seamless Multi-Computer Control Simulation, Live Telemetry & Tab Handlers
 */

document.addEventListener("DOMContentLoaded", () => {
  // 1. Theme Management (Dark / Light / System)
  initThemeToggle();

  // 2. Interactive Workstation Simulator
  initWorkstationSimulator();

  // 3. Screenshot Gallery Tabs
  initGalleryTabs();

  // 4. Code Block Tab Switcher & 1-Click Copy
  initCodeTabs();

  // 5. Header Scroll Effect
  initHeaderScroll();
});

/* ==========================================================================
   1. Theme Management
   ========================================================================== */
function initThemeToggle() {
  const toggleBtn = document.getElementById("theme-toggle");
  if (!toggleBtn) return;

  const savedTheme = localStorage.getItem("moush-theme") || "dark";
  document.documentElement.setAttribute("data-theme", savedTheme);

  toggleBtn.addEventListener("click", () => {
    const currentTheme = document.documentElement.getAttribute("data-theme") || "dark";
    const nextTheme = currentTheme === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", nextTheme);
    localStorage.setItem("moush-theme", nextTheme);
    updateThemeIcon(nextTheme);
  });

  updateThemeIcon(savedTheme);
}

function updateThemeIcon(theme) {
  const icon = document.getElementById("theme-toggle-icon");
  if (!icon) return;
  if (theme === "light") {
    // Show Moon for switching to dark
    icon.innerHTML = `<path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>`;
  } else {
    // Show Sun for switching to light
    icon.innerHTML = `<circle cx="12" cy="12" r="5" fill="none" stroke="currentColor" stroke-width="2"/><line x1="12" y1="1" x2="12" y2="3" stroke="currentColor" stroke-width="2"/><line x1="12" y1="21" x2="12" y2="23" stroke="currentColor" stroke-width="2"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64" stroke="currentColor" stroke-width="2"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78" stroke="currentColor" stroke-width="2"/><line x1="1" y1="12" x2="3" y2="12" stroke="currentColor" stroke-width="2"/><line x1="21" y1="12" x2="23" y2="12" stroke="currentColor" stroke-width="2"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36" stroke="currentColor" stroke-width="2"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22" stroke="currentColor" stroke-width="2"/>`;
  }
}

/* ==========================================================================
   2. Interactive Workstation Simulator
   ========================================================================== */
function initWorkstationSimulator() {
  const stage = document.getElementById("sim-stage");
  const cursor = document.getElementById("sim-cursor");
  const monitorDesktop = document.getElementById("monitor-desktop");
  const monitorLaptop = document.getElementById("monitor-laptop");
  const statusPill = document.getElementById("sim-status-text");
  const energyConduit = document.getElementById("sim-energy-conduit");
  const toastDesktop = document.getElementById("sim-toast-desktop");
  const toastLaptop = document.getElementById("sim-toast-laptop");

  const btnEdge = document.getElementById("sim-btn-edge");
  const btnClipboard = document.getElementById("sim-btn-clipboard");
  const btnFile = document.getElementById("sim-btn-file");
  const btnLayout = document.getElementById("sim-btn-layout");

  if (!stage || !cursor || !monitorDesktop || !monitorLaptop) return;

  let isManualInteraction = false;
  let autoAnimId = null;
  let layoutFlipped = false;

  // Track cursor position inside stage
  stage.addEventListener("mousemove", (e) => {
    isManualInteraction = true;
    cancelAnimationFrame(autoAnimId);

    const rect = stage.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    cursor.style.transform = `translate(${x - 4}px, ${y - 4}px)`;

    // Detect which screen contains the cursor
    updateScreenAffordance(x, rect.width);
  });

  stage.addEventListener("mouseleave", () => {
    isManualInteraction = false;
    startIdleAnimation();
  });

  function updateScreenAffordance(x, totalWidth) {
    const threshold = totalWidth * 0.55;
    const isOverLaptop = layoutFlipped ? x < totalWidth * 0.45 : x > threshold;

    if (isOverLaptop) {
      monitorLaptop.classList.add("active-monitor");
      monitorDesktop.classList.remove("active-monitor");
      energyConduit.classList.add("active");
      if (statusPill) {
        statusPill.innerHTML = `Controlled: <strong>Laptop Client</strong> · Focus active · ~42 µs latency`;
      }
    } else {
      monitorDesktop.classList.add("active-monitor");
      monitorLaptop.classList.remove("active-monitor");
      energyConduit.classList.remove("active");
      if (statusPill) {
        statusPill.innerHTML = `Controlled: <strong>Desktop Host</strong> · Physical Input Hub`;
      }
    }
  }

  // Simulator Interactive Action Buttons
  if (btnEdge) {
    btnEdge.addEventListener("click", () => {
      cancelAnimationFrame(autoAnimId);
      animateCrossScreenTransition();
    });
  }

  if (btnClipboard) {
    btnClipboard.addEventListener("click", () => {
      cancelAnimationFrame(autoAnimId);
      simulateClipboardSync();
    });
  }

  if (btnFile) {
    btnFile.addEventListener("click", () => {
      cancelAnimationFrame(autoAnimId);
      simulateFileTransfer();
    });
  }

  if (btnLayout) {
    btnLayout.addEventListener("click", () => {
      layoutFlipped = !layoutFlipped;
      if (layoutFlipped) {
        stage.style.flexDirection = "row-reverse";
        btnLayout.classList.add("active");
      } else {
        stage.style.flexDirection = "row";
        btnLayout.classList.remove("active");
      }
      showToast(toastDesktop, "Layout rearranged · Saved to layout.json");
    });
  }

  // Animation: Cross-Screen Transition
  function animateCrossScreenTransition() {
    const rect = stage.getBoundingClientRect();
    const startX = rect.width * 0.25;
    const targetX = rect.width * 0.78;
    const y = rect.height * 0.48;

    let progress = 0;
    function step() {
      progress += 0.025;
      const currentX = startX + (targetX - startX) * progress;
      cursor.style.transform = `translate(${currentX}px, ${y}px)`;
      updateScreenAffordance(currentX, rect.width);

      if (progress < 1) {
        requestAnimationFrame(step);
      } else {
        setTimeout(() => {
          showToast(toastLaptop, "Cursor entered laptop screen");
          setTimeout(startIdleAnimation, 2000);
        }, 300);
      }
    }
    requestAnimationFrame(step);
  }

  // Animation: Clipboard Sync
  function simulateClipboardSync() {
    showToast(toastDesktop, "📋 Copied on Desktop: https://github.com/rakibul-islam-hasib/moush");
    energyConduit.classList.add("active");

    setTimeout(() => {
      showToast(toastLaptop, "📋 Synced to Laptop: Ready to paste (Noise-XX)");
      energyConduit.classList.remove("active");
      setTimeout(startIdleAnimation, 2500);
    }, 1000);
  }

  // Animation: File Transfer
  function simulateFileTransfer() {
    showToast(toastDesktop, "📦 Sending file: cargo-release-v0.1.0.zip (14.2 MB)");
    energyConduit.classList.add("active");

    setTimeout(() => {
      showToast(toastLaptop, "✅ Received: cargo-release-v0.1.0.zip (Verified SHA256)");
      energyConduit.classList.remove("active");
      setTimeout(startIdleAnimation, 3000);
    }, 1200);
  }

  function showToast(elem, text) {
    if (!elem) return;
    elem.textContent = text;
    elem.classList.add("show");
    setTimeout(() => {
      elem.classList.remove("show");
    }, 2800);
  }

  // Idle organic roaming animation
  let angle = 0;
  function startIdleAnimation() {
    if (isManualInteraction) return;

    function idleLoop() {
      if (isManualInteraction) return;
      angle += 0.015;
      const rect = stage.getBoundingClientRect();
      const midX = rect.width * 0.5;
      const midY = rect.height * 0.48;
      const x = midX + Math.sin(angle) * (rect.width * 0.32);
      const y = midY + Math.cos(angle * 1.5) * (rect.height * 0.22);

      cursor.style.transform = `translate(${x}px, ${y}px)`;
      updateScreenAffordance(x, rect.width);
      autoAnimId = requestAnimationFrame(idleLoop);
    }
    cancelAnimationFrame(autoAnimId);
    autoAnimId = requestAnimationFrame(idleLoop);
  }

  // Start initial idle loop
  startIdleAnimation();
}

/* ==========================================================================
   3. Screenshot Gallery Tabs
   ========================================================================== */
function initGalleryTabs() {
  const tabs = document.querySelectorAll(".gallery-tab-btn");
  const img = document.getElementById("gallery-active-img");
  const title = document.getElementById("gallery-title");
  const desc = document.getElementById("gallery-desc");

  if (!tabs.length || !img) return;

  const galleryData = {
    home: {
      src: "assets/screenshots/home-dark.png",
      title: "Home Dashboard",
      desc: "Instant connection status, sharing toggle, and real-time activity feed."
    },
    layout: {
      src: "assets/screenshots/layout.png",
      title: "Screen Layout Arranger",
      desc: "Drag-and-drop monitor positioning matching your desk's physical setup."
    },
    transfers: {
      src: "assets/screenshots/transfers.png",
      title: "Encrypted File Transfers",
      desc: "Resumable, high-throughput drag-and-drop file streaming over direct TCP."
    },
    features: {
      src: "assets/screenshots/features.png",
      title: "Feature Permissions",
      desc: "Granular controls for clipboard, keyboard shortcuts, and device access."
    },
    devices: {
      src: "assets/screenshots/devices.png",
      title: "Paired Devices",
      desc: "6-digit mutual authentication, trusted peers, and remote device management."
    },
    diagnostics: {
      src: "assets/screenshots/diagnostics.png",
      title: "Diagnostics & Firewall",
      desc: "Real-time latency telemetry, firewall helper, and live network logs."
    }
  };

  tabs.forEach((tab) => {
    tab.addEventListener("click", () => {
      const key = tab.getAttribute("data-gallery");
      const data = galleryData[key];
      if (!data) return;

      tabs.forEach((t) => t.classList.remove("active"));
      tab.classList.add("active");

      img.style.opacity = "0.3";
      img.style.transform = "scale(0.99)";

      setTimeout(() => {
        img.src = data.src;
        if (title) title.textContent = data.title;
        if (desc) desc.textContent = data.desc;
        img.style.opacity = "1";
        img.style.transform = "scale(1)";
      }, 150);
    });
  });
}

/* ==========================================================================
   4. Code Block Tab Switcher & 1-Click Copy
   ========================================================================== */
function initCodeTabs() {
  const tabBtns = document.querySelectorAll(".code-tab-btn");
  const codeContent = document.getElementById("code-snippet-content");
  const copyBtn = document.getElementById("code-copy-btn");

  const snippets = {
    winget: "winget install Moush",
    installer: "# Download the latest standalone installer directly from GitHub Releases\nInvoke-WebRequest -Uri 'https://github.com/rakibul-islam-hasib/moush/releases/latest/download/Moush_x64-setup.exe' -OutFile 'Moush-setup.exe'\nStart-Process 'Moush-setup.exe'",
    cargo: "# Build & run from source (Rust 1.85+ and Node.js 20+ required)\ngit clone https://github.com/rakibul-islam-hasib/moush.git\ncd moush\nnpm install\nnpx tauri dev",
    cli: "# Run the headless CLI tool without UI\ncargo build --release -p moush-cli\n./target/release/moush-cli host --port 24800"
  };

  tabBtns.forEach((btn) => {
    btn.addEventListener("click", () => {
      const target = btn.getAttribute("data-code");
      if (!snippets[target] || !codeContent) return;

      tabBtns.forEach((b) => b.classList.remove("active"));
      btn.classList.add("active");
      codeContent.textContent = snippets[target];
    });
  });

  if (copyBtn && codeContent) {
    copyBtn.addEventListener("click", () => {
      const textToCopy = codeContent.textContent;
      navigator.clipboard.writeText(textToCopy).then(() => {
        const originalText = copyBtn.textContent;
        copyBtn.textContent = "✓ Copied!";
        copyBtn.style.borderColor = "var(--brand-cyan)";
        copyBtn.style.color = "var(--brand-cyan)";

        setTimeout(() => {
          copyBtn.textContent = originalText;
          copyBtn.style.borderColor = "";
          copyBtn.style.color = "";
        }, 2000);
      });
    });
  }

  // Quick winget snippet in hero
  const quickWinget = document.getElementById("hero-winget-copy");
  if (quickWinget) {
    quickWinget.addEventListener("click", () => {
      navigator.clipboard.writeText("winget install Moush").then(() => {
        const badge = quickWinget.querySelector(".copy-badge");
        if (badge) {
          const prev = badge.textContent;
          badge.textContent = "Copied!";
          badge.style.color = "var(--brand-cyan)";
          setTimeout(() => {
            badge.textContent = prev;
            badge.style.color = "";
          }, 2000);
        }
      });
    });
  }
}

/* ==========================================================================
   5. Header Scroll Effect
   ========================================================================== */
function initHeaderScroll() {
  const header = document.querySelector(".site-header");
  if (!header) return;

  window.addEventListener("scroll", () => {
    if (window.scrollY > 20) {
      header.classList.add("scrolled");
    } else {
      header.classList.remove("scrolled");
    }
  });
}
