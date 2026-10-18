interface ToggleSwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  id?: string;
  disabled?: boolean;
  /** Accessible name when no visible label is referenced. */
  label?: string;
  labelledBy?: string;
  describedBy?: string;
  /** Show the "On" / "Off" text next to the switch, as in Windows Settings. */
  showState?: boolean;
}

export function ToggleSwitch({
  checked,
  onChange,
  id,
  disabled,
  label,
  labelledBy,
  describedBy,
  showState = true,
}: ToggleSwitchProps) {
  return (
    <span className="toggle-field">
      {showState && (
        <span className="toggle-state" aria-hidden="true">
          {checked ? "On" : "Off"}
        </span>
      )}
      <button
        id={id}
        type="button"
        role="switch"
        className="toggle"
        aria-checked={checked}
        aria-label={label}
        aria-labelledby={labelledBy}
        aria-describedby={describedBy}
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span className="toggle-knob" />
      </button>
    </span>
  );
}
