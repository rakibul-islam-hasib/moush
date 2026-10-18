/** Every screen. "layout" lives under Devices, "diagnostics" under Settings. */
export type PageId = "home" | "devices" | "layout" | "features" | "transfers" | "settings" | "diagnostics";

/** Top-level entries of the navigation pane. */
export type NavSection = "home" | "devices" | "features" | "transfers" | "settings";

export const SECTION_OF: Record<PageId, NavSection> = {
  home: "home",
  devices: "devices",
  layout: "devices",
  features: "features",
  transfers: "transfers",
  settings: "settings",
  diagnostics: "settings",
};

/** Feature cards on the Features page that other pages can link to. */
export type FeatureAnchor = "clipboard" | "history" | "keyboard" | "transfer" | "notifications";

export interface Route {
  page: PageId;
  /** Expander to open and scroll to on the Features page. */
  anchor?: FeatureAnchor;
}

export type Navigate = (page: PageId, anchor?: FeatureAnchor) => void;

export const PAGE_IDS: readonly PageId[] = ["home", "devices", "layout", "features", "transfers", "settings", "diagnostics"];
