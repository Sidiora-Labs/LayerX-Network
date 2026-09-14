"use client";

import { useEffect, useState } from "react";

import { copyEntry } from "../../copy/runtime.ts";
import { formatCopy } from "../../copy/format.ts";
import type { HumanApiClient, NativeFeeAsset } from "../api/index.ts";
import { KitButton } from "../kit/control.tsx";
import { TextField } from "../kit/field.tsx";
import { nativeFeeBudget, validNativeFeeAsset, type FeeLimitInput } from "./native-fee-budget.ts";

export function useNativeFeeBudget(client: HumanApiClient) {
  const [asset, setAsset] = useState<NativeFeeAsset>();
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [limits, setLimits] = useState<FeeLimitInput>({ perAction: "", total: "", perPeriod: "" });
  useEffect(() => {
    let cancelled = false;
    void client.sessionFeePolicy().then((received) => {
      if (!validNativeFeeAsset(received)) throw new Error("Invalid native fee asset");
      if (!cancelled) setAsset(received);
    }).catch(() => { if (!cancelled) setFailed(true); });
    return () => { cancelled = true; };
  }, [client, attempt]);
  return {
    asset, failed, limits, budget: nativeFeeBudget(limits, asset),
    setLimit: (name: keyof FeeLimitInput, value: string) => {
      setLimits((previous) => ({ ...previous, [name]: value }));
    },
    retry: () => { setFailed(false); setAttempt((previous) => previous + 1); },
  };
}

export function NativeFeeBudgetFields({
  control,
  disabled,
}: Readonly<{ control: ReturnType<typeof useNativeFeeBudget>; disabled: boolean }>) {
  return (
    <fieldset className="flex flex-col gap-3" disabled={disabled}>
      <legend className="text-sm font-medium">{copyEntry("fees.limits.title").message}</legend>
      <p className="text-sm text-muted-foreground">{copyEntry("fees.limits.body").message}</p>
      {control.asset === undefined ? (
        <div role="status">
          <p>{copyEntry(control.failed ? "fees.limits.unavailable" : "fees.limits.loading").message}</p>
          {control.failed ? <KitButton variant="secondary" onClick={control.retry} type="button">
            {copyEntry("action.retry").message}
          </KitButton> : null}
        </div>
      ) : (
        <>
          {(["perAction", "total", "perPeriod"] as const).map((name) => (
            <TextField
              key={name}
              name={`fee-${name}`}
              label={formatCopy(`fees.limits.${name}`, { currency: control.asset?.currency ?? "" })}
              inputMode="decimal"
              autoComplete="off"
              required
              value={control.limits[name]}
              onChange={(event) => { control.setLimit(name, event.target.value); }}
            />
          ))}
          {(["perAction", "total", "perPeriod"] as const).every((name) => control.limits[name].length > 0)
            && control.budget === undefined
            ? <p role="alert">{copyEntry("fees.limits.invalid").message}</p> : null}
        </>
      )}
    </fieldset>
  );
}
