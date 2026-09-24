import type CspDev from 'csp-dev';

import config from 'configs/app';

const zetachainFeature = config.features.zetachain;

export function zetachain(): CspDev.DirectiveDescriptor {
  if (!zetachainFeature.isEnabled || !config.apis.zetachain?.socketEndpoint) {
    return {};
  }

  return {
    'connect-src': [
      `${ config.apis.zetachain.socketEndpoint }/websocket`,
      config.apis.zetachain.endpoint,
    ],
  };
}
