import BigNumber from 'bignumber.js';

import { ZERO } from 'toolkit/utils/consts';

import { DEFAULT_ACCURACY, DEFAULT_ACCURACY_USD } from './utils';

export interface Params {
  amount: string;
  decimals?: string | number | null;
  exchangeRate?: string | null;
  accuracy?: number;
  accuracyUsd?: number;
}

export default function calculateUsdValue({ amount, accuracy = DEFAULT_ACCURACY, accuracyUsd = DEFAULT_ACCURACY_USD, decimals, exchangeRate }: Params) {
  const valueBn = BigNumber(amount).div(BigNumber(10 ** Number(decimals || '0')));

  const valueStr = (() => {
    if (!accuracy) {
      return valueBn.toFormat();
    }
    const formattedValue = valueBn.dp(accuracy).toFormat();
    return formattedValue === '0' && !valueBn.isEqualTo(ZERO) ? `< 0.${ '0'.repeat(accuracy - 1) }1` : formattedValue;
  })();

  const usdBn = (() => {
    if (!exchangeRate) {
      return ZERO;
    }
    const exchangeRateBn = new BigNumber(exchangeRate);
    return valueBn.times(exchangeRateBn);
  })();

  const usdStr = (() => {
    if (!exchangeRate) {
      return undefined;
    }

    if (accuracyUsd && !usdBn.isEqualTo(0)) {
      const usdBnDp = usdBn.dp(accuracyUsd);
      return usdBnDp.isEqualTo(0) ? usdBn.precision(accuracyUsd).toFormat() : usdBnDp.toFormat();
    }
    return usdBn.toFormat();
  })();

  return {
    valueBn,
    valueStr,
    usdBn,
    usdStr,
  };
}
