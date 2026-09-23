const DECIMAL_INPUT = /^(0|[1-9][0-9]*)(?:\.([0-9]+))?$/u;
const EVM_ADDRESS = /^0x[0-9a-fA-F]{40}$/u;
const BYTES32 = /^0x[0-9a-fA-F]{64}$/u;

export const WEI_DECIMALS = 18;

/** Launched tokens and the launchpad quote denom use six display decimals. */
export const LAUNCHPAD_DECIMALS = 6;

/** Renders base units as a decimal string with `decimals` fractional digits, trailing zeros trimmed. */
export function formatUnits(amount: bigint, decimals: number): string {
  if (decimals === 0) {
    return amount.toString();
  }
  const negative = amount < 0n;
  const magnitude = negative ? -amount : amount;
  const scale = 10n ** BigInt(decimals);
  const whole = magnitude / scale;
  const fraction = (magnitude % scale).toString().padStart(decimals, "0").replace(/0+$/u, "");
  return `${negative ? "-" : ""}${whole.toString()}${fraction === "" ? "" : `.${fraction}`}`;
}

/** Parses a typed decimal into base units; undefined when it is not an exact non-negative amount. */
export function parseUnits(text: string, decimals: number): bigint | undefined {
  const match = DECIMAL_INPUT.exec(text.trim());
  if (match === null) {
    return undefined;
  }
  const whole = match[1] ?? "0";
  const fraction = match[2] ?? "";
  if (fraction.length > decimals) {
    return undefined;
  }
  return BigInt(whole) * 10n ** BigInt(decimals) + BigInt(fraction.padEnd(decimals, "0") || "0");
}

export function shortId(value: string): string {
  return value.length > 18 ? `${value.slice(0, 10)}…${value.slice(-6)}` : value;
}

export function isEvmAddress(value: string | undefined): value is string {
  return value !== undefined && EVM_ADDRESS.test(value);
}

export function isBytes32(value: string | undefined): value is string {
  return value !== undefined && BYTES32.test(value);
}

export function firstParam(value: string | string[] | undefined): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

export type MarketSearchParams = Readonly<Record<string, string | string[] | undefined>>;
