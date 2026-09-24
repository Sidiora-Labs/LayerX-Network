import type CspDev from 'csp-dev';

import config from 'configs/app';

export function rollbar(): CspDev.DirectiveDescriptor {
  if (!config.features.rollbar.isEnabled) {
    return {};
  }

  return {
    'connect-src': [
      'api.rollbar.com',
    ],
  };
}
