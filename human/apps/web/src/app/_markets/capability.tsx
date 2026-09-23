"use client";

import { useRouter } from "next/navigation";
import { useEffect, useRef, useState } from "react";

import type { ForkSurface } from "../../api/gateway";
import { InlineNotice } from "../../kit/surface";
import { readSurfaceCapability } from "./capability-action";

const CAPABILITY_POLL_MS = 15_000;

const SURFACE_LABEL: Readonly<Record<ForkSurface, string>> = {
  exchange: "The exchange",
  bridge: "The bridge",
  launchpad: "The launchpad",
};

/**
 * Polls the gateway's capabilities for one fork surface and re-renders the
 * route when it flips, so the write paths open the moment the precompile has
 * code, without a redeploy. While the surface is not live it says so.
 */
export function SurfaceCapabilityGate({
  surface,
  live,
  detail,
}: Readonly<{ surface: ForkSurface; live: boolean; detail: string | null }>) {
  const router = useRouter();
  const rendered = useRef(live);
  const [current, setCurrent] = useState({ live, detail });

  useEffect(() => {
    rendered.current = live;
    setCurrent({ live, detail });
  }, [live, detail]);

  useEffect(() => {
    let cancelled = false;
    const timer = window.setInterval(() => {
      void readSurfaceCapability(surface).then(
        (answer) => {
          if (cancelled) {
            return;
          }
          setCurrent(answer);
          if (answer.live !== rendered.current) {
            rendered.current = answer.live;
            router.refresh();
          }
        },
        () => {
          if (!cancelled) {
            setCurrent((previous) => ({ live: previous.live, detail: "The capability check did not answer." }));
          }
        },
      );
    }, CAPABILITY_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [surface, router]);

  if (current.live) {
    return null;
  }
  return (
    <InlineNotice tone="warning" role="status">
      {SURFACE_LABEL[surface]} is not yet live on this network. This page is read-only until the chain answers for it;
      history stays available below and writes open here automatically once it does.
      {current.detail === null ? null : ` (${current.detail})`}
    </InlineNotice>
  );
}
