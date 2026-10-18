// Browser-preview-only URL switches for looking at (and screenshotting) every screen:
//   ?page=home|devices|layout|features|transfers|settings|diagnostics
//   &nav=compact      start with the navigation pane collapsed
//   &expand=all|clipboard|history|keyboard|transfer|notifications   open Features expanders
//   &drag=1           show the drag-and-drop overlay
// Theme, motion and role are mock settings: see src/api/mock.ts (?theme=dark&mockRole=client).
// Inside Tauri (backend.mock === null) none of this is active.

import { backend } from "../api";
import { PAGE_IDS, type FeatureAnchor, type PageId } from "./navigation";

export interface DevParams {
  page: PageId | null;
  navCollapsed: boolean;
  expand: FeatureAnchor | "all" | null;
  drag: boolean;
}

const ANCHORS: readonly string[] = ["all", "clipboard", "history", "keyboard", "transfer", "notifications"];

function parse(): DevParams {
  const q = new URLSearchParams(window.location.search);
  const page = q.get("page");
  const expand = q.get("expand");
  return {
    page: page && (PAGE_IDS as readonly string[]).includes(page) ? (page as PageId) : null,
    navCollapsed: q.get("nav") === "compact",
    expand: expand && ANCHORS.includes(expand) ? (expand as FeatureAnchor | "all") : null,
    drag: q.get("drag") === "1",
  };
}

export const dev: DevParams | null = backend.mock ? parse() : null;
