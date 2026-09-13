"use client";

import { Card } from "@layerx/ui/components/card";
import { Input, type InputProps } from "@layerx/ui/components/input";
import { List, ListItem, SectionHeader, type ListItemProps } from "@layerx/ui/components/list";
import { SegmentedControl, type SegmentedControlProps } from "@layerx/ui/components/segmented-control";
import { Switch } from "@layerx/ui/components/switch";
import type { ComponentProps, ReactNode } from "react";

export function SettingsSection({
  title,
  children,
}: Readonly<{ title: ReactNode; children: ReactNode }>) {
  return (
    <Card elevation="outline" className="flex flex-col gap-3">
      <SectionHeader title={title} />
      <List>{children}</List>
    </Card>
  );
}

export type SettingsRowProps = ListItemProps;

export function SettingsRow(props: SettingsRowProps) {
  return <ListItem {...props} />;
}

export function SettingsSwitch(
  props: ComponentProps<typeof Switch> & Readonly<{ label: string }>,
) {
  const { label, ...switchProps } = props;
  return <Switch {...switchProps} aria-label={label} />;
}

export function SettingsTextInput(props: InputProps) {
  return <Input {...props} />;
}

export function SettingsSegmentedControl(props: SegmentedControlProps) {
  return <SegmentedControl {...props} />;
}
