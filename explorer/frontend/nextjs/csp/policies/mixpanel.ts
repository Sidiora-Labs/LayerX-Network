import type CspDev from 'csp-dev';

import config from 'configs/app';

export function mixpanel(): CspDev.DirectiveDescriptor {
  if (!config.features.mixpanel.isEnabled) {
    return {};
  }

  return {
    'connect-src': [
      '*.mixpanel.com',
    ],
  };
}
