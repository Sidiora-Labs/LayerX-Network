"use client";

import { useEffect, useState, type ReactNode } from "react";

import { copyEntry } from "../../copy/runtime.ts";
import { InlineNotice } from "../kit/surface";

export function OfflineBanner() {
  const [offline, setOffline] = useState(false);

  useEffect(() => {
    const update = () => { setOffline(!navigator.onLine); };
    update();
    window.addEventListener("online", update);
    window.addEventListener("offline", update);
    return () => {
      window.removeEventListener("online", update);
      window.removeEventListener("offline", update);
    };
  }, []);

  if (!offline) {
    return null;
  }
  return <InlineNotice tone="warning" role="status">{copyEntry("state.offline.banner").message}</InlineNotice>;
}

export function QueuedActionNotice({ children }: Readonly<{ children?: ReactNode }>) {
  return (
    <InlineNotice tone="warning" role="status">
      {children ?? copyEntry("queue.waiting").message}
    </InlineNotice>
  );
}

export {
  OfflineActionQueue,
  type MoneyOfflineAction,
  type QueueDecision,
  type QueueableOfflineAction,
} from "./queue";
