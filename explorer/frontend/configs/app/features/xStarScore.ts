import type { Feature } from './types';

import { getEnvValue } from '../utils';

const title = 'XStar score';
const url = getEnvValue('NEXT_PUBLIC_XSTAR_SCORE_URL');

const config: Feature<{ url: string }> = (() => {
  if (url) {
    return Object.freeze({
      title,
      url,
      isEnabled: true,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
