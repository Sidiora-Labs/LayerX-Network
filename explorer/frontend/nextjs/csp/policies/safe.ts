import type CspDev from 'csp-dev';

import config from 'configs/app';

export function safe(): CspDev.DirectiveDescriptor {
  if (!config.features.safe.isEnabled) {
    return {};
  }

  return {
    'connect-src': [
      '*.safe.global',
    ],
  };
}
