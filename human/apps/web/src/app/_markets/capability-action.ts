"use server";

import { surfaceCapability, type ForkSurface, type SurfaceCapability } from "../../api/gateway";

function isForkSurface(surface: string): surface is ForkSurface {
  return surface === "exchange" || surface === "bridge" || surface === "launchpad";
}

/** Re-reads one surface's capability through the gateway for the client poll. */
export async function readSurfaceCapability(surface: string): Promise<SurfaceCapability> {
  if (!isForkSurface(surface)) {
    return { live: false, detail: "Unknown surface." };
  }
  return surfaceCapability(surface);
}
