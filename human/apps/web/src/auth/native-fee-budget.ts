import type { NativeFeeAsset, NativeFeeBudget } from "../api/index.ts";

export interface FeeLimitInput {
  readonly perAction: string;
  readonly total: string;
  readonly perPeriod: string;
}

const MAX_AMOUNT = (1n << 128n) - 1n;

export function validNativeFeeAsset(asset: NativeFeeAsset): boolean {
  return /^[0-9a-f]{64}$/u.test(asset.asset_id)
    && !/^0+$/u.test(asset.asset_id)
    && /^[A-Z][A-Z0-9]{1,11}$/u.test(asset.currency)
    && Number.isSafeInteger(asset.decimals) && asset.decimals >= 0 && asset.decimals <= 38;
}

export function parseFeeAmount(input: string, decimals: number): bigint | undefined {
  if (!Number.isSafeInteger(decimals) || decimals < 0 || decimals > 38 || input.length > 80) {
    return undefined;
  }
  const match = /^(0|[1-9][0-9]*)(?:\.([0-9]+))?$/u.exec(input.trim());
  if (match === null || (match[2]?.length ?? 0) > decimals) return undefined;
  const amount = BigInt(match[1] ?? "0") * 10n ** BigInt(decimals)
    + BigInt((match[2] ?? "").padEnd(decimals, "0") || "0");
  return amount > 0n && amount <= MAX_AMOUNT ? amount : undefined;
}

export function nativeFeeBudget(
  input: FeeLimitInput,
  asset: NativeFeeAsset | undefined,
): NativeFeeBudget | undefined {
  if (asset === undefined || !validNativeFeeAsset(asset)) return undefined;
  const perAction = parseFeeAmount(input.perAction, asset.decimals);
  const total = parseFeeAmount(input.total, asset.decimals);
  const perPeriod = parseFeeAmount(input.perPeriod, asset.decimals);
  if (perAction === undefined || total === undefined || perPeriod === undefined
    || perAction > total || perAction > perPeriod) return undefined;
  return {
    asset_id: asset.asset_id,
    maximum_per_activity: perAction,
    maximum_total: total,
    period_length_ms: 86_400_000n,
    maximum_per_period: perPeriod,
  };
}

export function nativeFeeBudgetIdentity(budget: NativeFeeBudget | undefined): string {
  return budget === undefined ? "" : [budget.asset_id, budget.maximum_per_activity,
    budget.maximum_total, budget.period_length_ms, budget.maximum_per_period].join(":");
}
