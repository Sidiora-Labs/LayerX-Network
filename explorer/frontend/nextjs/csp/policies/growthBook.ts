import type CspDev from 'csp-dev';

import config from 'configs/app';

export function growthBook(): CspDev.DirectiveDescriptor {
  if (!config.features.growthBook.isEnabled) {
    return {};
  }

  return {
    'connect-src': [
      'cdn.growthbook.io',
    ],
  };
}
