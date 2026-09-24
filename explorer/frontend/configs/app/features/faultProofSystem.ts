import type { Feature } from './types';

import { getEnvValue } from '../utils';
import rollup from './rollup';

const title = 'Fault proof system';

const config: Feature<{ isEnabled: true }> = (() => {
  if (rollup.isEnabled && rollup.type === 'optimistic' && getEnvValue('NEXT_PUBLIC_FAULT_PROOF_ENABLED') === 'true') {
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
