export type PageId = "home" | "devices" | "layout" | "settings" | "diagnostics";

export type Navigate = (page: PageId) => void;
