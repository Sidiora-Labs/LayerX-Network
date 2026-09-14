"use client";

import { GlobalSearch } from "@layerx/ui/components/search";
import type { ComponentProps } from "react";

export type SearchProps = Omit<ComponentProps<typeof GlobalSearch>, "platform">;

export function MobileSearch(props: SearchProps) {
  return <GlobalSearch {...props} platform="mobile" />;
}

export function DesktopSearch(props: SearchProps) {
  return <GlobalSearch {...props} platform="desktop" />;
}
