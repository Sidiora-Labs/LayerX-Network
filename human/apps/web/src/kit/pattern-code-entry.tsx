"use client";

import { CodeEntry } from "@layerx/ui/components/code-entry";
import type { ComponentProps } from "react";

export type CodeEntryProps = Omit<ComponentProps<typeof CodeEntry>, "platform">;

export function MobileCodeEntry(props: CodeEntryProps) {
  return <CodeEntry {...props} platform="mobile" />;
}

export function DesktopCodeEntry(props: CodeEntryProps) {
  return <CodeEntry {...props} platform="desktop" />;
}
