import { useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { useElementSize } from "../hooks/useElementSize";
import { cx } from "../lib/cx";
import { Icon, type IconName } from "./Icon";

export interface TabItem<T extends string> {
  id: T;
  label: string;
  icon?: IconName;
}

interface TabsProps<T extends string> {
  items: readonly TabItem<T>[];
  value: T;
  onChange: (id: T) => void;
  label: string;
  /** id prefix for aria-controls: the panel must have id `${idBase}-panel`. */
  idBase: string;
}

/** Selector bar with an underline indicator that slides to the selected tab. */
export function Tabs<T extends string>({ items, value, onChange, label, idBase }: TabsProps<T>) {
  const listRef = useRef<HTMLDivElement>(null);
  const size = useElementSize(listRef);
  const [indicator, setIndicator] = useState<{ left: number; width: number } | null>(null);

  useLayoutEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(`[data-tab="${value}"]`);
    if (el) setIndicator({ left: el.offsetLeft + 12, width: Math.max(16, el.offsetWidth - 24) });
  }, [value, size.width]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const index = items.findIndex((i) => i.id === value);
    let next = -1;
    if (e.key === "ArrowRight") next = (index + 1) % items.length;
    else if (e.key === "ArrowLeft") next = (index - 1 + items.length) % items.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = items.length - 1;
    if (next < 0) return;
    e.preventDefault();
    onChange(items[next].id);
    listRef.current?.querySelector<HTMLElement>(`[data-tab="${items[next].id}"]`)?.focus();
  };

  return (
    <div className="tabs" role="tablist" aria-label={label} ref={listRef} onKeyDown={onKeyDown}>
      {items.map((item) => {
        const selected = item.id === value;
        return (
          <button
            key={item.id}
            type="button"
            role="tab"
            data-tab={item.id}
            id={`${idBase}-tab-${item.id}`}
            aria-selected={selected}
            aria-controls={`${idBase}-panel`}
            tabIndex={selected ? 0 : -1}
            className={cx("tab", selected && "is-selected")}
            onClick={() => onChange(item.id)}
          >
            {item.icon && <Icon name={item.icon} />}
            <span>{item.label}</span>
          </button>
        );
      })}
      {indicator && <span className="tabs-indicator" aria-hidden="true" style={{ transform: `translateX(${indicator.left}px)`, width: indicator.width }} />}
    </div>
  );
}
