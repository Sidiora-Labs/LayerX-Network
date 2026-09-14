"use client";

import { DetailDisclosure } from "@layerx/ui/components/detail";
import type { ComponentProps } from "react";

export type DetailProps = Omit<ComponentProps<typeof DetailDisclosure>, "platform">;

export function MobileDetail(props: DetailProps) {
  return <DetailDisclosure {...props} platform="mobile" />;
}

export function DesktopDetail(props: DetailProps) {
  return <DetailDisclosure {...props} platform="desktop" />;
}
