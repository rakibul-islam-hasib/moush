import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { applyMotion, applyTheme } from "./hooks/useTheme";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/controls.css";
import "./styles/shell.css";
import "./styles/pages.css";
import "./styles/screens.css";
import "./styles/layout-editor.css";

// No browser context menu (Save as, Print, Inspect...) in the app. Text fields keep theirs
// for cut/copy/paste; dev builds keep it everywhere for Inspect.
if (!import.meta.env.DEV) {
  document.addEventListener("contextmenu", (e) => {
    const target = e.target as Element | null;
    if (!target?.closest("input, textarea, [contenteditable='true']")) e.preventDefault();
  });
}

// Follow the OS theme until settings arrive, so there is no flash.
applyTheme("system");
applyMotion("system");

const root = document.getElementById("root");
if (!root) throw new Error("Missing #root element");

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
