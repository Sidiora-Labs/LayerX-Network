import type { Feature } from './types';

import { getEnvValue } from '../utils';
import rollup from './rollup';

const title = 'MUD framework';

const config: Feature<{ isEnabled: true }> = (() => {
  if (rollup.isEnabled && rollup.type === 'optimistic' && getEnvValue('NEXT_PUBLIC_HAS_MUD_FRAMEWORK') === 'true') {
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
