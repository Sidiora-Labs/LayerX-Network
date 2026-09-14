"use client";

import { List, ListItem } from "@layerx/ui/components/list";
import type { ReactNode } from "react";

export interface DeviceListItem {
  readonly id: string;
  readonly title: ReactNode;
  readonly subtitle: ReactNode;
  readonly trailing?: ReactNode;
  readonly trailingCaption?: ReactNode;
  readonly current: boolean;
}

export function DeviceSessionList({ items }: Readonly<{ items: readonly DeviceListItem[] }>) {
  return (
    <List className="mt-3" data-device-list="">
      {items.map((item) => (
        <ListItem
          key={item.id}
          title={item.title}
          subtitle={item.subtitle}
          trailing={item.trailing}
          trailingCaption={item.trailingCaption}
          data-current-device={item.current ? "true" : "false"}
        />
      ))}
    </List>
  );
}
