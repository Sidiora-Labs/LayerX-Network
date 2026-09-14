"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";

import { copyEntry } from "../../../copy/runtime";
import { formatCopy } from "../../../copy/format";
import { DesktopNotifications, MobileNotifications } from "../../kit/pattern-notifications";
import { InlineNotice, ScreenCard } from "../../kit/surface";
import { KitButton } from "../../kit/control";
import { useAuthenticatedShell } from "../../shell/app-shell";
import { errorPresentation, ErrorSurface } from "../../states/error";
import { LoadingSurface, OfflineSurface } from "../../states/surfaces";
import { notificationItems, type PresentedNotification } from "./model";
import { useNotificationCenter } from "./store";

export function NotificationsArchiveScreen() {
  const { shell } = useAuthenticatedShell();
  const center = useNotificationCenter();
  const router = useRouter();
  const [opening, setOpening] = useState<string | undefined>(undefined);
  const [openError, setOpenError] = useState<unknown>(undefined);

  if (center.state.status === "loading") {
    return <LoadingSurface rows={5} />;
  }
  if (center.state.status === "error" && !navigator.onLine) {
    return <OfflineSurface onRetry={() => { void center.refresh(); }} />;
  }
  if (center.state.status === "error") {
    return (
      <ErrorSurface
        error={errorPresentation(center.state.error)}
        route="/app/notifications"
        onRetry={() => { void center.refresh(); }}
        onReload={() => { window.location.reload(); }}
      />
    );
  }

  const items = notificationItems(center.state.notifications);
  const open = async (item: { id: string }) => {
    const notification = center.state.status === "ready"
      ? center.state.notifications.find((candidate) => candidate.source.notification_id === item.id)
      : undefined;
    if (notification === undefined || opening !== undefined) {
      return;
    }
    setOpening(item.id);
    setOpenError(undefined);
    try {
      const landing = await center.open(notification);
      router.push(landing.href);
    } catch (error) {
      setOpenError(error);
    } finally {
      setOpening(undefined);
    }
  };

  return (
    <ScreenCard
      title={copyEntry("notification.title").message}
      description={copyEntry("notification.preferences.description").message}
      dataApplication="notifications"
    >
      <div className="flex flex-col gap-4 pt-4">
        <div className="flex items-center justify-between gap-3">
          <p role="status" className="text-sm font-semibold text-muted-foreground">
            {formatCopy("notification.unread", { count: center.state.unreadCount })}
          </p>
          <KitButton variant="secondary" onClick={() => { router.push("/app/settings"); }}>
            {copyEntry("notification.preferences.title").message}
          </KitButton>
        </div>
        {opening === undefined ? null : (
          <InlineNotice>{copyEntry("state.loading.body").message}</InlineNotice>
        )}
        {openError === undefined ? null : (
          <InlineNotice tone="warning" role="alert">
            {copyEntry("error.notification.not-found").message}
          </InlineNotice>
        )}
        {shell === "mobile" ? (
          <MobileNotifications
            items={items}
            onBack={() => { router.back(); }}
            onItemClick={(item) => { void open(item); }}
          />
        ) : (
          <DesktopNotifications
            view="archive"
            items={items}
            onItemClick={(item) => { void open(item); }}
          />
        )}
      </div>
    </ScreenCard>
  );
}

export function notificationById(
  notifications: readonly PresentedNotification[],
  id: string,
): PresentedNotification | undefined {
  return notifications.find((notification) => notification.source.notification_id === id);
}
