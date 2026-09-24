import type { Feature } from './types';

import { getEnvValue } from '../utils';

const title = 'Beacon chain';

const config: Feature<{ currency: { symbol: string }; validatorUrlTemplate: string | undefined; withdrawalsOnly: boolean }> = (() => {
  if (getEnvValue('NEXT_PUBLIC_HAS_BEACON_CHAIN') === 'true') {
    const validatorUrlTemplate = getEnvValue('NEXT_PUBLIC_BEACON_CHAIN_VALIDATOR_URL_TEMPLATE');
    const withdrawalsOnly = getEnvValue('NEXT_PUBLIC_BEACON_CHAIN_WITHDRAWALS_ONLY') === 'true';
    return Object.freeze({
      title,
      isEnabled: true,
      currency: {
        symbol:
          getEnvValue('NEXT_PUBLIC_BEACON_CHAIN_CURRENCY_SYMBOL') ||
          getEnvValue('NEXT_PUBLIC_NETWORK_CURRENCY_SYMBOL') ||
          '', // maybe we need some other default value here
      },
      validatorUrlTemplate,
      withdrawalsOnly,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
