import { useEffect } from "react";
import type { Theme } from "../api";

const darkQuery = () => window.matchMedia("(prefers-color-scheme: dark)");

export function applyTheme(theme: Theme): void {
  const resolved = theme === "system" ? (darkQuery().matches ? "dark" : "light") : theme;
  document.documentElement.dataset.theme = resolved;
}

/** Keeps html[data-theme] in sync with the setting (and the OS when "system"). */
export function useThemeEffect(theme: Theme): void {
  useEffect(() => {
    applyTheme(theme);
    if (theme !== "system") return;
    const mq = darkQuery();
    const onChange = () => applyTheme("system");
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [theme]);
}

/** Native Mica backdrop: the page background must be transparent to let it show. */
export function useMicaEffect(mica: boolean): void {
  useEffect(() => {
    document.documentElement.classList.toggle("mica", mica);
  }, [mica]);
}
