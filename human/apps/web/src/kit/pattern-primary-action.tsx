"use client";

import { PrimaryAction } from "@layerx/ui/components/primary-action";
import { cn } from "@layerx/ui/cn";
import { useId, type ComponentProps } from "react";

import { DisabledReason, REDUCED_MOTION_CLASS, type ControlAvailability } from "./control";

type PrimaryActionBaseProps = Omit<
  ComponentProps<typeof PrimaryAction>,
  "platform" | "disabled" | "aria-describedby"
>;
export type KitPrimaryActionProps = PrimaryActionBaseProps & ControlAvailability;

function PrimaryActionFor({
  platform,
  disabled = false,
  disabledReason,
  className,
  ...props
}: KitPrimaryActionProps & Readonly<{ platform: "mobile" | "desktop" }>) {
  const reasonId = useId();
  return (
    <div className="flex flex-col">
      <PrimaryAction
        {...props}
        platform={platform}
        disabled={disabled}
        aria-describedby={disabled ? reasonId : undefined}
        className={cn(REDUCED_MOTION_CLASS, className)}
      />
      <DisabledReason id={reasonId}>{disabled ? disabledReason : undefined}</DisabledReason>
    </div>
  );
}

export function MobilePrimaryAction(props: KitPrimaryActionProps) {
  return <PrimaryActionFor {...props} platform="mobile" />;
}

export function DesktopPrimaryAction(props: KitPrimaryActionProps) {
  return <PrimaryActionFor {...props} platform="desktop" />;
}
