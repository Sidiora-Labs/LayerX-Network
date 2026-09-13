"use client";

import { Button, type ButtonProps } from "@layerx/ui/components/button";
import { cn } from "@layerx/ui/cn";
import { useId, type ReactNode } from "react";

export const REDUCED_MOTION_CLASS =
  "disabled:bg-surface-sunken disabled:text-faint-foreground motion-reduce:animate-none motion-reduce:transition-none";

export type ControlAvailability =
  | Readonly<{ disabled?: false; disabledReason?: never }>
  | Readonly<{ disabled: true; disabledReason: string }>;

export type KitButtonProps = Omit<ButtonProps, "disabled"> & ControlAvailability;

export function DisabledReason({ id, children }: Readonly<{ id: string; children?: ReactNode }>) {
  if (children === undefined) {
    return null;
  }
  return (
    <p id={id} className="mt-1.5 text-sm text-muted-foreground" role="note">
      {children}
    </p>
  );
}

export function KitButton({
  disabled = false,
  disabledReason,
  className,
  "aria-describedby": describedBy,
  ...props
}: KitButtonProps) {
  const generatedReasonId = useId();
  const reasonId = disabled ? generatedReasonId : undefined;
  const description = [describedBy, reasonId].filter(Boolean).join(" ") || undefined;

  return (
    <span className="inline-flex flex-col">
      <Button
        {...props}
        disabled={disabled}
        aria-describedby={description}
        className={cn(REDUCED_MOTION_CLASS, className)}
      />
      <DisabledReason id={generatedReasonId}>{disabled ? disabledReason : undefined}</DisabledReason>
    </span>
  );
}
