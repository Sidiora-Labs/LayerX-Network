"use client";

import { Input } from "@layerx/ui/components/input";
import { cn } from "@layerx/ui/cn";
import { useId, type ComponentProps } from "react";

type InputBaseProps = Omit<ComponentProps<typeof Input>, "id" | "aria-describedby" | "aria-invalid">;

export type TextFieldProps = InputBaseProps & Readonly<{
  label: string;
  errorMessage?: string | undefined;
}>;

export function TextField({ label, errorMessage, error, className, ...props }: TextFieldProps) {
  const fieldId = useId();
  const errorId = useId();
  const invalid = error === true || errorMessage !== undefined;
  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      <label htmlFor={fieldId} className="text-sm font-semibold text-foreground">
        {label}
      </label>
      <Input
        {...props}
        id={fieldId}
        error={invalid}
        aria-invalid={invalid || undefined}
        aria-describedby={errorMessage === undefined ? undefined : errorId}
      />
      {errorMessage === undefined ? null : (
        <p id={errorId} role="alert" className="text-sm text-destructive">
          {errorMessage}
        </p>
      )}
    </div>
  );
}
