import type { CSSProperties } from "react";

interface SliderProps {
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
  format: (value: number) => string;
  labelledBy?: string;
  describedBy?: string;
  id?: string;
}

export function Slider({ value, min, max, step, onChange, format, labelledBy, describedBy, id }: SliderProps) {
  const pct = max > min ? ((value - min) / (max - min)) * 100 : 0;
  const style: CSSProperties & Record<"--pct", string> = { "--pct": `${pct}%` };
  return (
    <span className="slider-field">
      <span className="slider-value" aria-hidden="true">
        {format(value)}
      </span>
      <input
        id={id}
        className="slider"
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        style={style}
        aria-labelledby={labelledBy}
        aria-describedby={describedBy}
        aria-valuetext={format(value)}
        onChange={(e) => onChange(Number(e.target.value))}
      />
    </span>
  );
}
