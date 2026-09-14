"use client";

import { AppShell } from "@layerx/ui/components/app-shell";
import type { ComponentProps } from "react";

export type NavigationProps = Omit<ComponentProps<typeof AppShell>, "platform">;

function Navigation({ platform, ...props }: NavigationProps & Readonly<{ platform: "mobile" | "desktop" }>) {
  return <AppShell {...props} platform={platform} />;
}

export function MobileNavigation(props: NavigationProps) {
  return <Navigation {...props} platform="mobile" />;
}

export function DesktopNavigation(props: NavigationProps) {
  return <Navigation {...props} platform="desktop" />;
}
