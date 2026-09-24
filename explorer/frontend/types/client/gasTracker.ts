export const GAS_UNITS = [
  'usd',
  'gwei',
] as const;

export type GasUnit = typeof GAS_UNITS[number];
