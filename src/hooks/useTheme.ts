import { useEffect, useSyncExternalStore } from "react";
import type { AppearanceSettings, Theme } from "../api";

const darkQuery = () => window.matchMedia("(prefers-color-scheme: dark)");
const reducedQuery = () => window.matchMedia("(prefers-reduced-motion: reduce)");

export function applyTheme(theme: Theme): void {
  const resolved = theme === "system" ? (darkQuery().matches ? "dark" : "light") : theme;
  document.documentElement.dataset.theme = resolved;
}

export function applyMotion(motion: AppearanceSettings["motion"]): void {
  const reduce = motion === "reduce" || (motion === "system" && reducedQuery().matches);
  document.documentElement.dataset.motion = reduce ? "reduce" : "full";
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

/** Keeps html[data-motion] in sync with settings.appearance.motion (and the OS when "system"). */
export function useMotionEffect(motion: AppearanceSettings["motion"]): void {
  useEffect(() => {
    applyMotion(motion);
    if (motion !== "system") return;
    const mq = reducedQuery();
    const onChange = () => applyMotion("system");
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [motion]);
}

/**
 * Translucent surfaces only when the native Mica backdrop is really applied
 * (platform.mica) and the user wants transparency; otherwise everything is opaque.
 */
export function useSurfaceEffect(mica: boolean, transparency: boolean): void {
  const on = mica && transparency;
  useEffect(() => {
    document.documentElement.classList.toggle("mica", on);
  }, [on]);
}

/** True when animations should play (reads html[data-motion]). */
function subscribeMotion(cb: () => void) {
  const observer = new MutationObserver(cb);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-motion"] });
  return () => observer.disconnect();
}

export function useReducedMotion(): boolean {
  return useSyncExternalStore(subscribeMotion, () => document.documentElement.dataset.motion === "reduce");
}
