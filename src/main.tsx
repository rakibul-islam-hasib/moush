import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { applyMotion, applyTheme } from "./hooks/useTheme";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/controls.css";
import "./styles/shell.css";
import "./styles/pages.css";
import "./styles/screens.css";
import "./styles/layout-editor.css";

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
