import { useId } from "react";

export function ChoiceField({
  label,
  value,
  options,
  onChange,
}: Readonly<{
  label: string;
  value: string;
  options: readonly Readonly<{ value: string; label: string; disabled?: boolean }>[];
  onChange: (value: string) => void;
}>) {
  const id = useId();
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={id} className="text-sm font-semibold text-foreground">
        {label}
      </label>
      <select
        id={id}
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
        className="h-11 rounded-md border border-border-strong bg-surface px-3 text-[15px] text-foreground"
      >
        {options.map((option) => (
          <option key={option.value} value={option.value} disabled={option.disabled === true}>
            {option.label}
          </option>
        ))}
      </select>
    </div>
  );
}
