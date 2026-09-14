import { copyEntry, human_copy_catalog } from "../../../copy/runtime";
import { formatCopy } from "../../../copy/format";
import { Fragment, createElement, type ReactNode } from "react";
import type {
  NotificationPage,
  NotificationSummary,
} from "../../api";
import type { KitNotificationItem } from "../../kit/pattern-notifications";
import { PrivateFigure } from "../../settings/privacy";
import { moneyLabel } from "../approvals/model";

export const NOTIFICATIONS_ROUTE = "/app/notifications";

export interface PresentedNotification {
  readonly source: NotificationSummary;
  readonly repeatCount: number;
}

function timestamp(notification: NotificationSummary): number {
  const value = Date.parse(notification.created_at);
  return Number.isFinite(value) ? value : 0;
}

function subjectKey(notification: NotificationSummary): string {
  return [
    notification.class,
    notification.approval_id ?? "",
    notification.journey_id ?? "",
    notification.agent_id ?? "",
    safeDeepLink(notification.deep_link) ?? "",
  ].join(":");
}

export function presentedNotifications(page: NotificationPage): readonly PresentedNotification[] {
  const collapsed = new Map<string, PresentedNotification>();
  for (const notification of page.groups.flatMap((group) => group.notifications)) {
    const key = subjectKey(notification);
    const existing = collapsed.get(key);
    if (existing === undefined) {
      collapsed.set(key, Object.freeze({ source: notification, repeatCount: 1 }));
      continue;
    }
    const source = timestamp(notification) > timestamp(existing.source)
      ? notification
      : existing.source;
    collapsed.set(key, Object.freeze({
      source: { ...source, read: source.read && existing.source.read },
      repeatCount: existing.repeatCount + 1,
    }));
  }
  return Object.freeze(
    [...collapsed.values()].sort((left, right) => timestamp(right.source) - timestamp(left.source)),
  );
}

export function unreadNotificationCount(notifications: readonly PresentedNotification[]): number {
  return notifications.filter((notification) => !notification.source.read).length;
}

export function safeDeepLink(value: string): string | undefined {
  if (!value.startsWith("/app/") || value.startsWith("//") || value.includes("\\")) {
    return undefined;
  }
  try {
    const url = new URL(value, "https://layerx.invalid");
    if (url.origin !== "https://layerx.invalid" || !url.pathname.startsWith("/app/")) {
      return undefined;
    }
    return `${url.pathname}${url.search}${url.hash}`;
  } catch {
    return undefined;
  }
}

function body(notification: PresentedNotification): ReactNode {
  const bodyKey = human_copy_catalog().has(notification.source.body_copy_key)
    ? notification.source.body_copy_key
    : `notification.${notification.source.class}.body`;
  const message = copyEntry(bodyKey).message;
  const amount = notification.source.money === undefined
    ? copyEntry("privacy.hidden").message
    : moneyLabel(notification.source.money);
  const marker = "{amount}";
  const markerAt = message.indexOf(marker);
  const repeated = notification.repeatCount > 1
    ? formatCopy("notification.repeats", { count: notification.repeatCount - 1 })
    : undefined;
  if (markerAt < 0) {
    const rendered = formatCopy(bodyKey);
    return repeated === undefined ? rendered : `${rendered} ${repeated}`;
  }
  return createElement(
    Fragment,
    null,
    message.slice(0, markerAt),
    createElement(PrivateFigure, null, amount),
    message.slice(markerAt + marker.length),
    repeated === undefined ? null : ` ${repeated}`,
  );
}

export function notificationItems(
  notifications: readonly PresentedNotification[],
): KitNotificationItem[] {
  return notifications.map((notification): KitNotificationItem => {
    const href = safeDeepLink(notification.source.deep_link);
    const item = {
      id: notification.source.notification_id,
      title: (human_copy_catalog().get(notification.source.title_copy_key)
        ?? copyEntry(`notification.${notification.source.class}.title`)).message,
      body: body(notification),
      date: new Date(notification.source.created_at),
      read: notification.source.read,
    };
    return href === undefined ? item : { ...item, href };
  });
}
