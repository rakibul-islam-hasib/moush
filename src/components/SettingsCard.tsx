import { useId, type ReactNode } from "react";
import { cx } from "../lib/cx";
import { Icon, type IconName } from "./Icon";

export interface CardIds {
  /** id of the visible title, for aria-labelledby on the control. */
  label: string;
  /** id of the description, for aria-describedby on the control. */
  desc: string;
}

interface SettingsCardProps {
  icon?: IconName;
  title: ReactNode;
  description?: ReactNode;
  /** Control on the right; pass a function to receive ids for labelling. */
  children?: ReactNode | ((ids: CardIds) => ReactNode);
  /** Extra content below the row (e.g. a validation message). */
  footer?: ReactNode;
  /** When the change takes effect, e.g. "Restarts sharing" (shown as a small note). */
  hint?: ReactNode;
  disabled?: boolean;
  className?: string;
}

/** A settings row: icon, title, description and a control. */
export function SettingsCard({ icon, title, description, children, footer, hint, disabled, className }: SettingsCardProps) {
  const base = useId();
  const ids: CardIds = { label: `${base}-label`, desc: `${base}-desc` };
  const control = typeof children === "function" ? children(ids) : children;
  return (
    <div className={cx("settings-card", disabled && "is-disabled", className)}>
      <div className="settings-card-row">
        {icon && <Icon name={icon} size={20} className="settings-card-icon" />}
        <div className="settings-card-text">
          <span className="settings-card-title" id={ids.label}>
            {title}
          </span>
          {description && (
            <span className="settings-card-desc" id={ids.desc}>
              {description}
            </span>
          )}
          {hint}
        </div>
        {control !== undefined && control !== null && <div className="settings-card-control">{control}</div>}
      </div>
      {footer}
    </div>
  );
}

interface LinkCardProps {
  icon: IconName;
  title: string;
  description?: ReactNode;
  /** Shown before the chevron (e.g. a count badge). */
  trailing?: ReactNode;
  onClick: () => void;
}

/** A whole-card button that navigates somewhere (chevron on the right). */
export function LinkCard({ icon, title, description, trailing, onClick }: LinkCardProps) {
  return (
    <button type="button" className="settings-card settings-card-link" onClick={onClick}>
      <span className="settings-card-row">
        <Icon name={icon} size={20} className="settings-card-icon" />
        <span className="settings-card-text">
          <span className="settings-card-title">{title}</span>
          {description && <span className="settings-card-desc">{description}</span>}
        </span>
        {trailing}
        <Icon name="chevronRight" className="settings-card-chevron" />
      </span>
    </button>
  );
}

interface CardGroupProps {
  title?: string;
  description?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}

/** A titled group of settings cards. */
export function CardGroup({ title, description, actions, children }: CardGroupProps) {
  const id = useId();
  return (
    <section className="card-group" aria-labelledby={title ? id : undefined}>
      {(title || actions) && (
        <div className="card-group-header">
          <div>
            {title && (
              <h2 className="card-group-title" id={id}>
                {title}
              </h2>
            )}
            {description && <p className="card-group-desc">{description}</p>}
          </div>
          {actions && <div className="card-group-actions">{actions}</div>}
        </div>
      )}
      <div className="card-stack">{children}</div>
    </section>
  );
}

/** A card containing a vertical list of rows separated by dividers. */
export function ListCard({ children, label }: { children: ReactNode; label?: string }) {
  return (
    <ul className="list-card" aria-label={label}>
      {children}
    </ul>
  );
}

interface ListRowProps {
  icon?: IconName;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  dimmed?: boolean;
}

export function ListRow({ icon, title, description, children, dimmed }: ListRowProps) {
  return (
    <li className={cx("list-row", dimmed && "is-dimmed")}>
      {icon && <Icon name={icon} size={20} className="settings-card-icon" />}
      <div className="settings-card-text">
        <span className="settings-card-title">{title}</span>
        {description && <span className="settings-card-desc">{description}</span>}
      </div>
      {children && <div className="settings-card-control">{children}</div>}
    </li>
  );
}

/** "Restarts sharing" / "Applies on reconnect": when a change takes effect. */
export function ApplyHint({ children, icon = "refresh" }: { children: ReactNode; icon?: IconName }) {
  return (
    <span className="apply-hint">
      <Icon name={icon} size={16} />
      {children}
    </span>
  );
}

interface EmptyStateProps {
  icon: IconName;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
}

/** Friendly placeholder for an empty list. */
export function EmptyState({ icon, title, children, action }: EmptyStateProps) {
  return (
    <div className="empty-state">
      <span className="empty-state-icon">
        <Icon name={icon} size={24} />
      </span>
      <span className="empty-state-title">{title}</span>
      {children && <span className="empty-state-text">{children}</span>}
      {action && <div className="empty-state-action">{action}</div>}
    </div>
  );
}
