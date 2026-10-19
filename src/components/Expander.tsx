import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { cx } from "../lib/cx";
import { Icon, type IconName } from "./Icon";

interface ExpanderProps {
  icon?: IconName;
  title: ReactNode;
  description?: ReactNode;
  /** Control shown in the header next to the chevron (e.g. the feature's master switch). */
  headerControl?: ReactNode | ((ids: { label: string; desc: string }) => ReactNode);
  children: ReactNode;
  /** Uncontrolled initial state. */
  defaultOpen?: boolean;
  /** Controlled state. */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  id?: string;
  className?: string;
}

/**
 * Settings expander: a card whose header toggles a body with more settings. The body
 * animates its height (CSS grid 0fr -> 1fr) and is removed from the tab order and the
 * accessibility tree while collapsed (inert).
 */
export function Expander({ icon, title, description, headerControl, children, defaultOpen = false, open, onOpenChange, id, className }: ExpanderProps) {
  const [inner, setInner] = useState(defaultOpen);
  const isOpen = open ?? inner;
  const base = useId();
  const ids = { label: `${base}-label`, desc: `${base}-desc`, body: `${base}-body` };
  const bodyRef = useRef<HTMLDivElement>(null);

  const toggle = () => {
    const next = !isOpen;
    if (open === undefined) setInner(next);
    onOpenChange?.(next);
  };

  // `inert` keeps collapsed content out of the tab order (React 19 supports the attribute).
  useEffect(() => {
    bodyRef.current?.toggleAttribute("inert", !isOpen);
  }, [isOpen]);

  const control = typeof headerControl === "function" ? headerControl(ids) : headerControl;

  return (
    <section className={cx("expander", isOpen && "is-open", className)} id={id} aria-labelledby={ids.label}>
      {/* The whole header is a pointer target; the chevron button is the accessible control. */}
      <div
        className="expander-header"
        onClick={(e) => {
          if ((e.target as HTMLElement).closest(".expander-header-control, .expander-toggle")) return;
          toggle();
        }}
      >
        {icon && <Icon name={icon} size={20} className="settings-card-icon" />}
        <div className="settings-card-text">
          <h3 className="settings-card-title expander-title" id={ids.label}>
            {title}
          </h3>
          {description && (
            <span className="settings-card-desc" id={ids.desc}>
              {description}
            </span>
          )}
        </div>
        {control !== undefined && control !== null && <div className="expander-header-control">{control}</div>}
        <button
          type="button"
          className="btn btn-subtle btn-icon expander-toggle"
          aria-expanded={isOpen}
          aria-controls={ids.body}
          aria-labelledby={ids.label}
          aria-describedby={description ? ids.desc : undefined}
          onClick={toggle}
        >
          <Icon name="chevronDown" className="expander-chevron" />
        </button>
      </div>
      <div className="expander-body" id={ids.body} ref={bodyRef}>
        <div className="expander-body-inner">{children}</div>
      </div>
    </section>
  );
}

interface ExpanderRowProps {
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode | ((ids: { label: string; desc: string }) => ReactNode);
  /** Extra content below the row (validation message, list...). */
  footer?: ReactNode;
  disabled?: boolean;
  className?: string;
}

/** A row inside an expander body (indented to the header text, divider above). */
export function ExpanderRow({ title, description, children, footer, disabled, className }: ExpanderRowProps) {
  const base = useId();
  const ids = { label: `${base}-label`, desc: `${base}-desc` };
  const control = typeof children === "function" ? children(ids) : children;
  return (
    <div className={cx("expander-row", disabled && "is-disabled", className)}>
      <div className="expander-row-main">
        <div className="settings-card-text">
          <span className="settings-card-title" id={ids.label}>
            {title}
          </span>
          {description && (
            <span className="settings-card-desc" id={ids.desc}>
              {description}
            </span>
          )}
        </div>
        {control !== undefined && control !== null && <div className="settings-card-control">{control}</div>}
      </div>
      {footer}
    </div>
  );
}
