import { Icon } from "./Icon";

export interface SelectOption<T extends string> {
  value: T;
  label: string;
}

interface SelectProps<T extends string> {
  value: T;
  options: readonly SelectOption<T>[];
  onChange: (value: T) => void;
  id?: string;
  label?: string;
  labelledBy?: string;
  describedBy?: string;
}

/** Fluent-styled ComboBox built on the native <select> (native dropdown, full keyboard support). */
export function Select<T extends string>({ value, options, onChange, id, label, labelledBy, describedBy }: SelectProps<T>) {
  return (
    <span className="select-wrap">
      <select
        id={id}
        className="select"
        value={value}
        aria-label={label}
        aria-labelledby={labelledBy}
        aria-describedby={describedBy}
        onChange={(e) => {
          const next = options.find((o) => o.value === e.target.value);
          if (next) onChange(next.value);
        }}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <Icon name="chevronDown" className="select-chevron" />
    </span>
  );
}
