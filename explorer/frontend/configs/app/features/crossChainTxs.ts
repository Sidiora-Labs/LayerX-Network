import type { Feature } from './types';

import apis from '../apis';
import { getEnvValue } from '../utils';

const title = 'Cross-chain transactions';

const config: Feature<{}> = (() => {
  if (getEnvValue('NEXT_PUBLIC_CROSS_CHAIN_TXS_ENABLED') === 'true' && apis.interchainIndexer) {
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
