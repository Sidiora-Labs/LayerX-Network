import type { Feature } from './types';

import apis from '../apis';
import { getEnvValue } from '../utils';

const isEnabled = getEnvValue('NEXT_PUBLIC_MULTICHAIN_ENABLED') === 'true';
const cluster = getEnvValue('NEXT_PUBLIC_MULTICHAIN_CLUSTER');

const title = 'Multichain explorer';

const config: Feature<{ cluster: string }> = (() => {
  if (apis.multichainAggregator && apis.multichainStats && isEnabled && cluster) {
    return Object.freeze({
      title,
      isEnabled: true,
      cluster,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
