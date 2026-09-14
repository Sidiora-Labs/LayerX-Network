"use client";

import { Wizard } from "@layerx/ui/components/wizard";
import type { ComponentProps } from "react";

export type WizardProps = Omit<ComponentProps<typeof Wizard>, "platform">;

export function MobileWizard(props: WizardProps) {
  return <Wizard {...props} platform="mobile" />;
}

export function DesktopWizard(props: WizardProps) {
  return <Wizard {...props} platform="desktop" />;
}
