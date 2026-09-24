import type { Feature } from './types';

import apis from '../apis';
import { getEnvValue } from '../utils';

const dexPoolsEnabled = getEnvValue('NEXT_PUBLIC_DEX_POOLS_ENABLED') === 'true';

const title = 'DEX Pools';

const config: Feature<{ }> = (() => {
  if (apis.contractInfo && dexPoolsEnabled) {
    return Object.freeze({
      title,
      isEnabled: true,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
