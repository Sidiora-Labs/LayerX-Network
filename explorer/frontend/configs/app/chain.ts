import type { RollupType } from 'types/client/rollup';
import type { AdditionalTokenType } from 'types/client/token';
import type { NetworkVerificationType, NetworkVerificationTypeEnvs } from 'types/networks';

import { urlValidator } from 'toolkit/components/forms/validators/url';

import { getEnvValue, parseEnvJson } from './utils';

const DEFAULT_CURRENCY_DECIMALS = 18;

const rollupType = getEnvValue('NEXT_PUBLIC_ROLLUP_TYPE') as RollupType;

const verificationType: NetworkVerificationType = (() => {
  if (rollupType === 'arbitrum') {
    return 'posting';
  }
  if (rollupType === 'zkEvm') {
    return 'sequencing';
  }
  return getEnvValue('NEXT_PUBLIC_NETWORK_VERIFICATION_TYPE') as NetworkVerificationTypeEnvs || 'mining';
})();

const rpcUrls = (() => {
  const envValue = getEnvValue('NEXT_PUBLIC_NETWORK_RPC_URL');
  const isUrl = urlValidator(envValue);

  if (envValue && isUrl === true) {
    return [ envValue ];
  }

  const parsedValue = parseEnvJson<Array<string>>(envValue);

  return Array.isArray(parsedValue) ? parsedValue : [];
})();

const chain = Object.freeze({
  id: getEnvValue('NEXT_PUBLIC_NETWORK_ID'),
  name: getEnvValue('NEXT_PUBLIC_NETWORK_NAME'),
  shortName: getEnvValue('NEXT_PUBLIC_NETWORK_SHORT_NAME'),
  currency: {
    name: getEnvValue('NEXT_PUBLIC_NETWORK_CURRENCY_NAME'),
    weiName: getEnvValue('NEXT_PUBLIC_NETWORK_CURRENCY_WEI_NAME'),
    gweiName: getEnvValue('NEXT_PUBLIC_NETWORK_CURRENCY_GWEI_NAME'),
    symbol: getEnvValue('NEXT_PUBLIC_NETWORK_CURRENCY_SYMBOL'),
    decimals: Number(getEnvValue('NEXT_PUBLIC_NETWORK_CURRENCY_DECIMALS')) || DEFAULT_CURRENCY_DECIMALS,
  },
  secondaryCoin: {
    symbol: getEnvValue('NEXT_PUBLIC_NETWORK_SECONDARY_COIN_SYMBOL'),
  },
  hasMultipleGasCurrencies: getEnvValue('NEXT_PUBLIC_NETWORK_MULTIPLE_GAS_CURRENCIES') === 'true',
  tokenStandard: getEnvValue('NEXT_PUBLIC_NETWORK_TOKEN_STANDARD_NAME') || 'ERC',
  additionalTokenTypes: parseEnvJson<Array<AdditionalTokenType>>(getEnvValue('NEXT_PUBLIC_NETWORK_ADDITIONAL_TOKEN_TYPES')) || [],
  rpcUrls,
  isTestnet: getEnvValue('NEXT_PUBLIC_IS_TESTNET') === 'true',
  verificationType,
});

export default chain;
