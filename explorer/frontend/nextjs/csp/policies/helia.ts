import type CspDev from 'csp-dev';

import config from 'configs/app';

import { KEY_WORDS } from '../utils';

export function helia(): CspDev.DirectiveDescriptor {
  if (!config.UI.views.nft.verifiedFetch.isEnabled) {
    return {};
  }

  return {
    'connect-src': [
      'https://delegated-ipfs.dev',
      'https://trustless-gateway.link',
    ],
    'img-src': [
      KEY_WORDS.BLOB,
    ],
  };
}
