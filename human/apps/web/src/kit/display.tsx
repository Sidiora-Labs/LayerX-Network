"use client";

import { Badge, type BadgeProps } from "@layerx/ui/components/badge";
import { BalanceHeader } from "@layerx/ui/components/balance-header";
import { IconTile, List, ListItem, SectionHeader, ViewAllChip, type ListItemProps } from "@layerx/ui/components/list";
import { Input, type InputProps } from "@layerx/ui/components/input";
import { OptionList, type OptionListItem } from "@layerx/ui/components/option-list";
import { QuickActions } from "@layerx/ui/components/quick-actions";
import { formatRecency } from "@layerx/ui/lib/format";
import { useId, type ComponentProps, type ReactNode } from "react";

import { protocolAmount, type ProtocolAmount } from "./model";

export { formatRecency };
export type { OptionListItem };

export type KitListProps = ComponentProps<typeof List>;
export type KitListItemProps = ListItemProps;
export type KitSectionHeaderProps = ComponentProps<typeof SectionHeader>;
export type KitOptionListProps = ComponentProps<typeof OptionList>;
export type KitIconTileProps = ComponentProps<typeof IconTile>;
export type KitViewAllChipProps = ComponentProps<typeof ViewAllChip>;

export function KitList(props: KitListProps) {
  return <List {...props} />;
}

export function KitListItem(props: KitListItemProps) {
  return <ListItem {...props} />;
}

export function KitSectionHeader(props: KitSectionHeaderProps) {
  return <SectionHeader {...props} />;
}

export function KitOptionList(props: KitOptionListProps) {
  return <OptionList {...props} />;
}

export function KitIconTile(props: KitIconTileProps) {
  return <IconTile {...props} />;
}

export function KitViewAllChip(props: KitViewAllChipProps) {
  return <ViewAllChip {...props} />;
}

export type CountBadgeProps = Omit<BadgeProps, "children"> & Readonly<{ label: string }>;

export function CountBadge({ label, ...props }: CountBadgeProps) {
  return (
    <Badge role="status" aria-live="polite" aria-atomic="true" {...props}>
      {label}
    </Badge>
  );
}

export interface BalanceSummaryProps {
  readonly label: string;
  readonly value: ProtocolAmount;
  readonly currency: string;
  readonly hidden: boolean;
  readonly onHiddenChange: (hidden: boolean) => void;
  readonly className?: string;
}

export function BalanceSummary({
  label,
  value,
  currency,
  hidden,
  onHiddenChange,
  className,
}: BalanceSummaryProps) {
  return (
    <BalanceHeader
      label={label}
      value={protocolAmount(value)}
      symbol={currency}
      hidden={hidden}
      onHiddenChange={onHiddenChange}
      {...(className === undefined ? {} : { className })}
    />
  );
}

export type KitTextFieldProps = Omit<InputProps, "id" | "placeholder"> &
  Readonly<{ label: string }>;

export function KitTextField({ label, ...props }: KitTextFieldProps) {
  const fieldId = useId();
  return (
    <div className="flex flex-col gap-2">
      <label htmlFor={fieldId} className="text-sm font-semibold text-foreground">
        {label}
      </label>
      <Input id={fieldId} {...props} />
    </div>
  );
}

const ACTION_ICONS = {
  add: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </svg>
  ),
  move: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M5 12h14" />
      <path d="m12 5 7 7-7 7" />
    </svg>
  ),
  withdraw: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M12 5v14" />
      <path d="m19 12-7 7-7-7" />
    </svg>
  ),
} as const satisfies Readonly<Record<string, ReactNode>>;

export type ActionGridIcon = keyof typeof ACTION_ICONS;

export interface ActionGridItem {
  readonly id: string;
  readonly label: string;
  readonly icon: ActionGridIcon;
}

export interface ActionGridProps {
  readonly actions: readonly ActionGridItem[];
  readonly onAction: (id: string) => void;
  readonly className?: string;
}

export function ActionGrid({ actions, onAction, className }: ActionGridProps) {
  return (
    <QuickActions
      actions={actions.map((action) => ({
        id: action.id,
        label: action.label,
        icon: ACTION_ICONS[action.icon],
      }))}
      onAction={onAction}
      {...(className === undefined ? {} : { className })}
    />
  );
}
