import "server-only";

import { createHumanApiClient, type HumanApiClient } from "./generated/index.ts";

export interface ServerHumanApiContext {
  readonly cookie: string;
  readonly csrfToken?: string;
  readonly trace?: string;
}

export function humanApiForServer(context: ServerHumanApiContext): HumanApiClient {
  const baseUrl = process.env.LAYERX_HUMAN_SERVICE_URL;
  if (baseUrl === undefined) {
    throw new Error("LAYERX_HUMAN_SERVICE_URL must name the HTTPS human service");
  }
  const endpoint = new URL(baseUrl);
  if (
    endpoint.protocol !== "https:" ||
    endpoint.username !== "" ||
    endpoint.password !== "" ||
    endpoint.pathname !== "/" ||
    endpoint.search !== "" ||
    endpoint.hash !== ""
  ) {
    throw new Error("LAYERX_HUMAN_SERVICE_URL must name the HTTPS human service");
  }
  const origin = process.env.LAYERX_HUMAN_WEB_ORIGIN;
  if (origin === undefined || new URL(origin).origin !== origin || !origin.startsWith("https://")) {
    throw new Error("LAYERX_HUMAN_WEB_ORIGIN must name the HTTPS web application");
  }
  return createHumanApiClient({
    baseUrl: endpoint.origin,
    credentials: "include",
    headers: { Cookie: context.cookie, Origin: origin },
    csrfToken: () => context.csrfToken,
    trace: () => context.trace,
  });
}
