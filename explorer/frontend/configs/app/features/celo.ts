import type { Feature } from './types';

import { getEnvValue } from '../utils';

const title = 'Celo chain';

const config: Feature<{ }> = (() => {

  if (getEnvValue('NEXT_PUBLIC_CELO_ENABLED') === 'true') {
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
