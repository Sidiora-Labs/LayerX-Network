import type { ControlAvailability } from "../../kit/control";

/** The first reason an action is unavailable, as KitButton availability props. */
export function availability(...reasons: readonly (string | false | undefined)[]): ControlAvailability {
  const reason = reasons.find((candidate): candidate is string => typeof candidate === "string");
  return reason === undefined ? {} : { disabled: true, disabledReason: reason };
}
