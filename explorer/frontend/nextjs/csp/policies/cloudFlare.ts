import type CspDev from 'csp-dev';

import { KEY_WORDS } from '../utils';

// CloudFlare analytics
export function cloudFlare(): CspDev.DirectiveDescriptor {
  return {
    'script-src': [
      'static.cloudflareinsights.com',
    ],
    'style-src': [
      KEY_WORDS.DATA,
    ],
  };
}
