import { copyEntry } from "../../copy/runtime.ts";

export function human_web_app_scaffold() {
  return Object.freeze({
    application: copyEntry("application.name").message,
    planes: ["/app", "/explorer"] as const,
  });
}
