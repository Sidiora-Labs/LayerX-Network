import type { Feature } from './types';

import { getEnvValue } from '../utils';

const badgeClaimLink = getEnvValue('NEXT_PUBLIC_GAME_BADGE_CLAIM_LINK');

const title = 'Easter egg badge';

const config: Feature<{ badgeClaimLink: string }> = (() => {
  if (badgeClaimLink) {
    return Object.freeze({
      title,
      isEnabled: true,
      badgeClaimLink,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
