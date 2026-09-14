"use client";

import { MoneyList } from "@layerx/ui/components/money-list";
import type { ComponentProps } from "react";

export type MoneyListProps = Omit<ComponentProps<typeof MoneyList>, "platform">;

export function MobileMoneyList(props: MoneyListProps) {
  return <MoneyList {...props} platform="mobile" />;
}

export function DesktopMoneyList(props: MoneyListProps) {
  return <MoneyList {...props} platform="desktop" />;
}
