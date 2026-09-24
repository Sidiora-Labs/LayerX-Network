import type { Feature } from './types';
import { VALIDATORS_CHAIN_TYPE } from 'types/client/validators';
import type { ValidatorsChainType } from 'types/client/validators';

import { getEnvValue } from '../utils';

const chainType = ((): ValidatorsChainType | undefined => {
  const envValue = getEnvValue('NEXT_PUBLIC_VALIDATORS_CHAIN_TYPE') as ValidatorsChainType | undefined;
  return envValue && VALIDATORS_CHAIN_TYPE.includes(envValue) ? envValue : undefined;
})();

const title = 'Validators list';

const config: Feature<{ chainType: ValidatorsChainType }> = (() => {
  if (chainType) {
    return Object.freeze({
      title,
      isEnabled: true,
      chainType,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
