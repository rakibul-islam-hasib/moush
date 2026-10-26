// Moush website: theme toggle, header state, copy buttons and the interactive desk demo.

const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");

initTheme();
initHeader();
initCopyButtons();
initDesk();

/* ---------------------------------------------------------------- Theme */

function initTheme() {
  const button = document.getElementById("theme-toggle");
  const root = document.documentElement;
  const label = () => {
    const next = root.dataset.theme === "dark" ? "light" : "dark";
    button.setAttribute("aria-label", `Switch to ${next} theme`);
  };
  label();
  button.addEventListener("click", () => {
    root.dataset.theme = root.dataset.theme === "dark" ? "light" : "dark";
    try {
      localStorage.setItem("moush-theme", root.dataset.theme);
    } catch {
      // Storage can be unavailable (private windows); the toggle still works for this visit.
    }
    label();
  });
}

/* ---------------------------------------------------------------- Header */

function initHeader() {
  const header = document.getElementById("site-header");
  const update = () => header.classList.toggle("is-scrolled", window.scrollY > 8);
  update();
  window.addEventListener("scroll", update, { passive: true });
}

/* ---------------------------------------------------------------- Copy */

function initCopyButtons() {
  for (const button of document.querySelectorAll("[data-copy]")) {
    const label = button.querySelector("span");
    button.addEventListener("click", async () => {
      const text = document.querySelector(button.dataset.copy)?.textContent ?? "";
      try {
        await navigator.clipboard.writeText(text);
        label.textContent = "Copied";
        button.classList.add("is-done");
      } catch {
        label.textContent = "Select and copy";
      }
      setTimeout(() => {
        label.textContent = "Copy";
        button.classList.remove("is-done");
      }, 1600);
    });
  }
}

/* ---------------------------------------------------------------- Desk demo
 *
 * Two displays share one edge: the laptop's right side and the monitor's left side,
 * over the height where they overlap. The cursor crosses only through that segment,
 * like Moush does; elsewhere it stops at the screen edge. Typing goes to whichever
 * computer has the cursor.
 */

function initDesk() {
  const desk = document.getElementById("desk");
  if (!desk) return;

  const pcs = {
    laptop: desk.querySelector('[data-pc="laptop"]'),
    desktop: desk.querySelector('[data-pc="desktop"]'),
  };
  const names = { laptop: "Laptop", desktop: "Desktop" };
  const cursor = document.getElementById("fake-cursor");
  const status = document.getElementById("desk-active");

  let rects = null; // display rectangles in desk coordinates
  let active = "desktop";
  let rel = { u: 0.5, v: 0.5 }; // cursor position as a fraction of the active display
  let interacted = false;
  let autoplay = null;
  const fresh = new Set(["laptop", "desktop"]); // screens still showing their sample text

  function measure() {
    const base = desk.getBoundingClientRect();
    const box = (pc) => {
      const r = pcs[pc].querySelector(".display").getBoundingClientRect();
      return { left: r.left - base.left, top: r.top - base.top, right: r.right - base.left, bottom: r.bottom - base.top };
    };
    const L = box("laptop");
    const M = box("desktop");
    const top = Math.max(L.top, M.top);
    const bottom = Math.min(L.bottom, M.bottom);
    rects = { laptop: L, desktop: M, top, bottom, mid: (L.right + M.left) / 2 };
    desk.style.setProperty("--edge-top", `${top}px`);
    desk.style.setProperty("--edge-h", `${Math.max(0, bottom - top)}px`);
    desk.style.setProperty("--edge-laptop-x", `${L.right - 1.5}px`);
    desk.style.setProperty("--edge-monitor-x", `${M.left - 1.5}px`);
    render();
  }

  function render() {
    const r = rects[active];
    const x = r.left + rel.u * (r.right - r.left);
    const y = r.top + rel.v * (r.bottom - r.top);
    // The arrow's tip sits at (5.5, 3.2) on the icon's 20 px grid.
    const s = cursor.getBoundingClientRect().width;
    cursor.style.transform = `translate(${x - (s * 5.5) / 20}px, ${y - (s * 3.2) / 20}px)`;
  }

  function setActive(pc) {
    if (pc === active) return;
    active = pc;
    for (const [name, el] of Object.entries(pcs)) el.classList.toggle("is-active", name === pc);
    status.textContent = names[pc];
    desk.classList.remove("crossed");
    void desk.offsetWidth; // restart the edge flash
    desk.classList.add("crossed");
  }

  // Moves the cursor towards a point in desk coordinates.
  function point(px, py) {
    const inShared = py >= rects.top && py <= rects.bottom;
    const side = px < rects.mid ? "laptop" : "desktop";
    if (side !== active && inShared) setActive(side);
    const r = rects[active];
    const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), hi);
    rel = {
      u: (clamp(px, r.left, r.right - 1) - r.left) / (r.right - r.left),
      v: (clamp(py, r.top, r.bottom - 1) - r.top) / (r.bottom - r.top),
    };
    render();
  }

  function jumpTo(pc) {
    setActive(pc);
    rel = { u: 0.5, v: 0.55 };
    render();
  }

  function type(pc, text) {
    const target = pcs[pc].querySelector("[data-typed]");
    if (fresh.has(pc)) {
      fresh.delete(pc);
      target.textContent = "";
    }
    target.textContent = (target.textContent + text).slice(-90);
  }

  function takeOver() {
    interacted = true;
    desk.classList.add("is-live");
    if (autoplay) autoplay.cancel();
  }

  const local = (e) => {
    const base = desk.getBoundingClientRect();
    return [e.clientX - base.left, e.clientY - base.top];
  };

  desk.addEventListener("pointermove", (e) => {
    if (e.pointerType === "touch") return;
    takeOver();
    point(...local(e));
  });

  desk.addEventListener("pointerdown", (e) => {
    takeOver();
    point(...local(e));
  });

  desk.addEventListener("keydown", (e) => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      takeOver();
      jumpTo(e.key === "ArrowLeft" ? "laptop" : "desktop");
    } else if (e.key === "Backspace") {
      takeOver();
      const target = pcs[active].querySelector("[data-typed]");
      fresh.delete(active);
      target.textContent = target.textContent.slice(0, -1);
    } else if (e.key.length === 1) {
      takeOver();
      type(active, e.key);
    } else {
      return;
    }
    e.preventDefault();
  });

  // One short, explanatory run when the desk first comes into view: the cursor glides
  // to the laptop, the desktop's keyboard types there, and it comes back. Stops the
  // moment the visitor moves, clicks or types; never runs with reduced motion.
  function play() {
    let cancelled = false;
    let frame = 0;
    let timer = 0;
    const wait = (ms) => new Promise((resolve) => (timer = setTimeout(resolve, ms)));
    const ease = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
    const glide = (from, to, ms) =>
      new Promise((resolve) => {
        const start = performance.now();
        const step = (now) => {
          if (cancelled) return resolve();
          const t = Math.min(1, (now - start) / ms);
          const k = ease(t);
          point(from[0] + (to[0] - from[0]) * k, from[1] + (to[1] - from[1]) * k);
          if (t < 1) frame = requestAnimationFrame(step);
          else resolve();
        };
        frame = requestAnimationFrame(step);
      });

    (async () => {
      const y = (rects.top + rects.bottom) / 2;
      const onMonitor = [rects.desktop.left + (rects.desktop.right - rects.desktop.left) * 0.55, y];
      const onLaptop = [rects.laptop.left + (rects.laptop.right - rects.laptop.left) * 0.45, y];
      await wait(500);
      if (cancelled) return;
      await glide(onMonitor, onLaptop, 1300);
      for (const ch of "Typed with the desktop keyboard") {
        if (cancelled) return;
        type("laptop", ch);
        await wait(45);
      }
      await wait(700);
      if (cancelled) return;
      await glide(onLaptop, onMonitor, 1300);
    })();

    return {
      cancel() {
        cancelled = true;
        cancelAnimationFrame(frame);
        clearTimeout(timer);
      },
    };
  }

  measure();
  pcs.desktop.classList.add("is-active");
  point(rects.desktop.left + (rects.desktop.right - rects.desktop.left) * 0.55, (rects.top + rects.bottom) / 2);

  new ResizeObserver(measure).observe(desk);

  if (!reducedMotion.matches && "IntersectionObserver" in window) {
    const io = new IntersectionObserver(
      (entries) => {
        if (!entries.some((e) => e.isIntersecting)) return;
        io.disconnect();
        if (!interacted) autoplay = play();
      },
      { threshold: 0.6 },
    );
    io.observe(desk);
  }
}
