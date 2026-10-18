import { useEffect, useState, type RefObject } from "react";

export interface Size {
  width: number;
  height: number;
}

/** Tracks an element's content box size with a ResizeObserver. */
export function useElementSize(ref: RefObject<HTMLElement | null>): Size {
  const [size, setSize] = useState<Size>({ width: 0, height: 0 });
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      const box = entries[0]?.contentRect;
      if (!box) return;
      setSize((prev) =>
        prev.width === box.width && prev.height === box.height ? prev : { width: box.width, height: box.height },
      );
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref]);
  return size;
}
