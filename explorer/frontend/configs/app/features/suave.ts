import type { Feature } from './types';

import { getEnvValue } from '../utils';

const title = 'SUAVE chain';

const config: Feature<{ isEnabled: true }> = (() => {
  if (getEnvValue('NEXT_PUBLIC_IS_SUAVE_CHAIN') === 'true') {
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
