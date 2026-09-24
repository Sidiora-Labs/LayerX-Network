import type CspDev from 'csp-dev';

import config from 'configs/app';

const feature = config.features.addressProfileAPI;

export function usernameApi(): CspDev.DirectiveDescriptor {
  if (!feature.isEnabled) {
    return {};
  }

  const apiOrigin = (() => {
    try {
      const url = new URL(feature.apiUrlTemplate);
      return url.origin;
    } catch (error) {
      return '';
    }
  })();

  return {
    'connect-src': [
      apiOrigin,
    ],
  };
}
