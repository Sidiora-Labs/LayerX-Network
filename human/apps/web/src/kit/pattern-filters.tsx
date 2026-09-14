"use client";

import { FilterBar } from "@layerx/ui/components/filters";
import type { ComponentProps } from "react";

export type FiltersProps = Omit<ComponentProps<typeof FilterBar>, "platform">;

export function MobileFilters(props: FiltersProps) {
  return <FilterBar {...props} platform="mobile" />;
}

export function DesktopFilters(props: FiltersProps) {
  return <FilterBar {...props} platform="desktop" />;
}
