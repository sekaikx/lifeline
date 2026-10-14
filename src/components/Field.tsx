import { useEffect, useId, useState, type ReactNode } from "react";

interface NumberInputProps {
  value: number;
  onChange: (v: number) => void;
  unit?: string;
  min?: number;
  max?: number;
  step?: number;
  integer?: boolean;
  ariaLabel?: string;
  id?: string;
}

/** Number box that keeps the user's text while typing and only commits valid values. */
export function NumberInput({ value, onChange, unit, min, max, step, integer, ariaLabel, id }: NumberInputProps) {
  const [text, setText] = useState(String(value));
  const [focused, setFocused] = useState(false);

  useEffect(() => {
    if (!focused) setText(formatValue(value));
  }, [value, focused]);

  const parse = (s: string): number | null => {
    const v = Number(s);
    if (s.trim() === "" || !Number.isFinite(v)) return null;
    if (integer && !Number.isInteger(v)) return null;
    if (min !== undefined && v < min) return null;
    if (max !== undefined && v > max) return null;
    return v;
  };
  const valid = parse(text) !== null;

  return (
    <div className={`input-wrap${valid ? "" : " invalid"}`}>
      <input
        id={id}
        aria-label={ariaLabel}
        inputMode="decimal"
        value={text}
        step={step}
        onFocus={() => setFocused(true)}
        onBlur={() => {
          setFocused(false);
          setText(formatValue(value));
        }}
        onChange={(e) => {
          setText(e.target.value);
          const v = parse(e.target.value);
          if (v !== null && v !== value) onChange(v);
        }}
        onKeyDown={(e) => {
          if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
          e.preventDefault();
          const d = (step ?? 1) * (e.shiftKey ? 10 : 1) * (e.key === "ArrowUp" ? 1 : -1);
          let v = Number((value + d).toPrecision(12));
          if (min !== undefined) v = Math.max(min, v);
          if (max !== undefined) v = Math.min(max, v);
          setText(formatValue(v));
          onChange(v);
        }}
      />
      {unit && <span className="unit">{unit}</span>}
    </div>
  );
}

function formatValue(v: number): string {
  return Number.isInteger(v) ? String(v) : String(Number(v.toPrecision(8)));
}

interface FieldProps extends Omit<NumberInputProps, "id" | "ariaLabel"> {
  label: string;
  hint?: string;
}

export function NumberField({ label, hint, ...rest }: FieldProps) {
  const id = useId();
  return (
    <div className="field">
      <label htmlFor={id}>
        {label}
        {hint && <span className="hint">{hint}</span>}
      </label>
      <NumberInput id={id} {...rest} />
    </div>
  );
}

export function SelectField<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  const id = useId();
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <select id={id} className="select" value={value} onChange={(e) => onChange(e.target.value as T)}>
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </div>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  ariaLabel,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  ariaLabel?: string;
}) {
  return (
    <div className="segmented" role="group" aria-label={ariaLabel}>
      {options.map((o) => (
        <button key={o.value} type="button" aria-pressed={o.value === value} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Section({ title, children, open = true }: { title: string; children: ReactNode; open?: boolean }) {
  return (
    <details className="section" open={open}>
      <summary>{title}</summary>
      <div className="section-body">{children}</div>
    </details>
  );
}
