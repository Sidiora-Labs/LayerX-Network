import type CspDev from 'csp-dev';

import config from 'configs/app';

import { KEY_WORDS } from '../utils';

const feature = config.features.blockchainInteraction;

export function blockchainInteraction(): CspDev.DirectiveDescriptor {
  if (!feature.isEnabled) {
    return {};
  }

  switch (feature.connectorType) {
    case 'reown': {
      return {
        'connect-src': [
          '*.web3modal.com',
          '*.web3modal.org',
          '*.walletconnect.com',
          '*.walletconnect.org',
          'wss://relay.walletconnect.com',
          'wss://relay.walletconnect.org',
          'wss://www.walletlink.org',
        ],
        'frame-ancestors': [
          '*.walletconnect.org',
          '*.walletconnect.com',
        ],
        'img-src': [
          KEY_WORDS.BLOB,
          '*.walletconnect.com',
        ],
      };
    }
    case 'dynamic': {
      return {
        'connect-src': [
          'https://dynamic-static-assets.com',
          'https://app.dynamicauth.com',
          'https://logs.dynamicauth.com',
          'wss://relay.walletconnect.com',
          'wss://relay.walletconnect.org',
        ],
        'font-src': [
          'https://cdn.jsdelivr.net/npm/@fontsource/dm-sans/files/dm-sans-latin-400-normal.woff2',
          'https://cdn.jsdelivr.net/npm/@fontsource/dm-sans/files/dm-sans-latin-400-normal.woff',
          'https://cdn.jsdelivr.net/npm/@fontsource/dm-sans/files/dm-sans-latin-400-italic.woff2',
          'https://cdn.jsdelivr.net/npm/@fontsource/dm-sans/files/dm-sans-latin-400-italic.woff',
          'https://cdn.jsdelivr.net/npm/@fontsource/dm-sans/files/dm-sans-latin-500-normal.woff2',
          'https://cdn.jsdelivr.net/npm/@fontsource/dm-sans/files/dm-sans-latin-500-normal.woff',
          'https://cdn.jsdelivr.net/npm/@fontsource/dm-sans/files/dm-sans-latin-500-italic.woff2',
          'https://cdn.jsdelivr.net/npm/@fontsource/dm-sans/files/dm-sans-latin-500-italic.woff',
          'https://cdn.jsdelivr.net/npm/@fontsource/dm-sans/files/dm-sans-latin-700-normal.woff2',
          'https://cdn.jsdelivr.net/npm/@fontsource/dm-sans/files/dm-sans-latin-700-normal.woff',
          'https://cdn.jsdelivr.net/npm/@fontsource/dm-sans/files/dm-sans-latin-700-italic.woff2',
          'https://cdn.jsdelivr.net/npm/@fontsource/dm-sans/files/dm-sans-latin-700-italic.woff',
        ],
        'style-src': [
          'https://app.dynamic.xyz',
        ],
        'img-src': [
          '*.walletconnect.com',
        ],
      };
    }
  }
}
