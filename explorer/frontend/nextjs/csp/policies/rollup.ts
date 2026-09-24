import type CspDev from 'csp-dev';

import config from 'configs/app';

const rollupFeature = config.features.rollup;

export function rollup(): CspDev.DirectiveDescriptor {
  if (!rollupFeature.isEnabled) {
    return {};
  }

  return {
    'connect-src': [
      ...(rollupFeature.parentChain.rpcUrls ?? []),
    ],
  };
}
