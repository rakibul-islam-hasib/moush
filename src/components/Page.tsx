import { useEffect, useRef, type ReactNode } from "react";
import { Icon } from "./Icon";

interface PageProps {
  title: string;
  /** One line under the title. */
  subtitle?: ReactNode;
  /** Breadcrumb parent ("Settings › Diagnostics"). */
  parent?: { label: string; onClick: () => void };
  actions?: ReactNode;
  children: ReactNode;
  /** Don't move focus to the title on mount (e.g. when only a tab changed). */
  keepFocus?: boolean;
}

/** Page frame with the title. Focus moves to the title when the page opens. */
export function Page({ title, subtitle, parent, actions, children, keepFocus = false }: PageProps) {
  const titleRef = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    if (!keepFocus) titleRef.current?.focus({ preventScroll: true });
  }, [keepFocus]);
  return (
    <div className="page">
      <header className="page-header">
        <div className="page-heading">
          {parent ? (
            <h1 className="page-title page-breadcrumb" tabIndex={-1} ref={titleRef}>
              <button type="button" className="breadcrumb-parent" onClick={parent.onClick}>
                {parent.label}
              </button>
              <Icon name="chevronRight" size={20} className="breadcrumb-sep" />
              <span aria-current="page">{title}</span>
            </h1>
          ) : (
            <h1 className="page-title" tabIndex={-1} ref={titleRef}>
              {title}
            </h1>
          )}
          {subtitle && <p className="page-subtitle">{subtitle}</p>}
        </div>
        {actions && <div className="page-actions">{actions}</div>}
      </header>
      {children}
    </div>
  );
}
