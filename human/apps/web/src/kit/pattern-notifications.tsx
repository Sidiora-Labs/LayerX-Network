"use client";

import { NotificationsArchive, NotificationsScreen, BellPopover } from "@layerx/ui/components/notifications";
import type { ComponentProps } from "react";

type MobileNotificationsPrimitiveProps = ComponentProps<typeof NotificationsScreen>;
type DesktopArchivePrimitiveProps = ComponentProps<typeof NotificationsArchive>;
type DesktopPopoverPrimitiveProps = ComponentProps<typeof BellPopover>;

export type KitNotificationItem = MobileNotificationsPrimitiveProps["items"][number];
export type MobileNotificationsProps = MobileNotificationsPrimitiveProps;
export type DesktopNotificationsProps =
  | (DesktopArchivePrimitiveProps & Readonly<{ view: "archive" }>)
  | (DesktopPopoverPrimitiveProps & Readonly<{ view: "popover" }>);

export function MobileNotifications(props: MobileNotificationsProps) {
  return <NotificationsScreen {...props} />;
}

export function DesktopNotifications({ view, ...props }: DesktopNotificationsProps) {
  return view === "archive" ? (
    <NotificationsArchive {...props} />
  ) : (
    <BellPopover {...props} />
  );
}
