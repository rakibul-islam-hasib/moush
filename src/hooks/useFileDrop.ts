import { useEffect, useRef, useState } from "react";
import { inTauri } from "../api/backend";

/**
 * Files dragged onto the window. Inside Tauri the webview reports absolute paths
 * (onDragDropEvent); in the browser preview the HTML5 drop gives only names, so
 * paths are simulated for the mock backend. Returns true while files hover the window.
 */
export function useFileDrop(onDrop: (paths: string[]) => void, enabled = true): boolean {
  const [over, setOver] = useState(false);
  const onDropRef = useRef(onDrop);
  useEffect(() => {
    onDropRef.current = onDrop;
  });

  useEffect(() => {
    if (!enabled) {
      setOver(false);
      return;
    }
    if (inTauri) {
      let unlisten: (() => void) | null = null;
      let disposed = false;
      // Loaded on demand: keeps the window/webview API out of the startup bundle.
      import("@tauri-apps/api/webview")
        .then(({ getCurrentWebview }) => getCurrentWebview().onDragDropEvent((event) => {
          const p = event.payload;
          if (p.type === "enter") setOver(p.paths.length > 0);
          else if (p.type === "leave") setOver(false);
          else if (p.type === "drop") {
            setOver(false);
            if (p.paths.length > 0) onDropRef.current(p.paths);
          }
        }))
        .then((fn) => {
          if (disposed) fn();
          else unlisten = fn;
        })
        .catch(() => {
          // Drag and drop is a convenience; "Send files…" still works without it.
        });
      return () => {
        disposed = true;
        unlisten?.();
      };
    }

    // Browser preview.
    let depth = 0;
    const hasFiles = (e: DragEvent) => Array.from(e.dataTransfer?.types ?? []).includes("Files");
    const enter = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      depth++;
      setOver(true);
    };
    const overHandler = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    };
    const leave = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      depth = Math.max(0, depth - 1);
      if (depth === 0) setOver(false);
    };
    const drop = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      depth = 0;
      setOver(false);
      const names = Array.from(e.dataTransfer?.files ?? []).map((f) => f.name);
      if (names.length > 0) onDropRef.current(names.map((n) => `C:\\Users\\you\\Desktop\\${n}`));
    };
    window.addEventListener("dragenter", enter);
    window.addEventListener("dragover", overHandler);
    window.addEventListener("dragleave", leave);
    window.addEventListener("drop", drop);
    return () => {
      window.removeEventListener("dragenter", enter);
      window.removeEventListener("dragover", overHandler);
      window.removeEventListener("dragleave", leave);
      window.removeEventListener("drop", drop);
    };
  }, [enabled]);

  return over;
}
