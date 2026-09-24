import type CspDev from 'csp-dev';

import config from 'configs/app';

const feature = config.features.megaEth;

export function megaEth(): CspDev.DirectiveDescriptor {
  if (!feature.isEnabled) {
    return {};
  }

  return {
    'connect-src': [
      feature.socketUrl.metrics,
      feature.socketUrl.rpc,
    ].filter(Boolean),
  };
}
