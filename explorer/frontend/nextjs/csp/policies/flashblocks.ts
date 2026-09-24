import type CspDev from 'csp-dev';

import config from 'configs/app';

const flashblocksFeature = config.features.flashblocks;

export function flashblocks(): CspDev.DirectiveDescriptor {
  if (!flashblocksFeature.isEnabled) {
    return {};
  }

  return {
    'connect-src': [
      flashblocksFeature.socketUrl,
    ],
  };
}
