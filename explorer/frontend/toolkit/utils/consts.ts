import BigNumber from 'bignumber.js';

export const ZERO = new BigNumber(0);

export const SECOND = 1_000;
export const MINUTE = 60 * SECOND;
export const HOUR = 60 * MINUTE;
export const DAY = 24 * HOUR;
export const WEEK = 7 * DAY;
export const MONTH = 30 * DAY;
export const YEAR = 365 * DAY;

export const Kb = 1_000;
export const Mb = 1_000 * Kb;

export const ZERO_ADDRESS = '0x0000000000000000000000000000000000000000';
