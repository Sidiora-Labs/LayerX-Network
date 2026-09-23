"use server";

import { surfaceCapability, type ForkSurface, type SurfaceCapability } from "../../api/gateway";

/** Re-reads one surface's capability through the gateway for the client poll. */
export async function readSurfaceCapability(surface: ForkSurface): Promise<SurfaceCapability> {
  if (surface !== "exchange" && surface !== "bridge" && surface !== "launchpad") {
    return { live: false, detail: "Unknown surface." };
  }
  return surfaceCapability(surface);
}
