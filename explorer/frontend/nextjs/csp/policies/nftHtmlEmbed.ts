import type CspDev from 'csp-dev';

import { KEY_WORDS } from '../utils';

/**
 * Content-Security-Policy used **only** for responses to `/nft-html-embed.html` (see `nextjs/csp/index.ts`).
 *
 * **What it is for**
 * Interactive NFTs often ship as `data:text/html` or remote HTML with arbitrary inline and third-party
 * scripts. The main explorer uses a strict `script-src` (hashed / `'self'` only). Any preview that
 * inherits that policy—e.g. `srcdoc`, same-origin `blob:`, or framing `data:` under the main document—
 * will block those scripts. We avoid widening CSP on the whole app by serving a tiny **first-party shell**
 * on this URL. The shell listens for `postMessage` from the explorer and sets a **nested** iframe’s
 * `src` to the NFT URL (commonly `data:text/html;base64,...`).
 *
 * **Why this policy is broad**
 * The **untrusted** markup runs inside the **inner** iframe (see `public/nft-html-embed.html`), not in
 * the shell’s inline script. The inner document still needs a permissive policy to run typical NFT
 * animations (inline scripts, eval in some bundles, arbitrary `connect-src`, images, fonts, nested
 * frames). This file’s CSP applies to the **shell document** and to what that document is allowed to
 * embed; we keep `object-src` and `base-uri` tight where we can.
 *
 * **`frame-src` includes `data:`**
 * In CSP, `frame-src *` does not cover `data:` URLs; those must be listed explicitly or framing fails.
 *
 * **`frame-ancestors 'self'`**
 * Only our origin may frame this page, reducing drive-by embedding and tightening who can target the
 * shell with `postMessage` (the shell also checks `event.origin` in script).
 */
export function nftHtmlEmbed(): CspDev.DirectiveDescriptor {
  return {
    'default-src': [ KEY_WORDS.NONE ],

    'script-src': [
      KEY_WORDS.UNSAFE_INLINE,
      KEY_WORDS.UNSAFE_EVAL,
      KEY_WORDS.SELF,
      KEY_WORDS.DATA,
      KEY_WORDS.BLOB,
      '*',
    ],

    'style-src': [
      KEY_WORDS.UNSAFE_INLINE,
      KEY_WORDS.SELF,
      '*',
    ],

    'img-src': [ KEY_WORDS.DATA, KEY_WORDS.BLOB, '*' ],

    'font-src': [ KEY_WORDS.DATA, KEY_WORDS.SELF, '*' ],

    'connect-src': [ '*' ],

    'media-src': [ KEY_WORDS.BLOB, '*' ],

    'object-src': [ KEY_WORDS.NONE ],

    'base-uri': [ KEY_WORDS.NONE ],

    'frame-ancestors': [ KEY_WORDS.SELF ],

    'frame-src': [ '*', KEY_WORDS.DATA ],
  };
}
