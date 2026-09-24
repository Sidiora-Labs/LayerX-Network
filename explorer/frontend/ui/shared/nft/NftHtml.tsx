import { Box, chakra } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';

import type { MediaElementProps } from './utils';

interface Props extends MediaElementProps<'div'> {
  fullscreen?: boolean;
}

const NFT_HTML_EMBED_PATH = '/nft-html-embed.html';

const NftHtml = ({ src, transport, onLoad, onError, onClick, fullscreen, ...rest }: Props) => {
  const ref = React.useRef<HTMLIFrameElement>(null);

  const [ isLoaded, setIsLoaded ] = React.useState(false);

  const handleMesageFromFrame = React.useCallback((event: MessageEvent) => {
    if (event.source !== ref.current?.contentWindow) {
      return;
    }
    if (event.origin !== window.location.origin) {
      return;
    }
    if (event.data?.type !== 'nft-html-embed-loaded' || event.data?.src !== src) {
      return;
    }
    setIsLoaded(true);
    onLoad?.();
  }, [ onLoad, src ]);

  const handleFrameLoad = React.useCallback(() => {
    ref.current?.contentWindow?.postMessage(
      { type: 'nft-html-embed', src }, window.location.origin,
    );
  }, [ src ]);

  const loadViaHttp = React.useCallback(() => {
    if (!ref.current) {
      return;
    }

    window.addEventListener('message', handleMesageFromFrame);

    ref.current.onload = handleFrameLoad;
    ref.current.src = `${ window.location.origin }${ NFT_HTML_EMBED_PATH }`;
    onError && (ref.current.onerror = onError);
  }, [ handleFrameLoad, handleMesageFromFrame, onError ]);

  React.useEffect(() => {
    // Disable iframe in private mode to prevent tracking
    if (config.app.isPrivateMode) {
      onError?.();
      return;
    }

    switch (transport) {
      case 'ipfs': {
        // Currently we don't support IPFS video loading
        onError?.();
        break;
      }
      case 'http': {
        loadViaHttp();
        break;
      }
    }

    const iframe = ref.current;

    return () => {
      if (transport === 'http') {
        window.removeEventListener('message', handleMesageFromFrame);
        if (iframe) {
          iframe.onload = null;
          iframe.onerror = null;
        }
      }
    };
  }, [ handleMesageFromFrame, loadViaHttp, onError, transport ]);

  // Disable iframe in private mode to prevent tracking
  if (config.app.isPrivateMode) {
    return null;
  }

  return (
    <Box
      h="100%"
      w="100%"
      position="relative"
      onClick={ onClick }
      _before={ !fullscreen ? {
        content: '""',
        position: 'absolute',
        top: 0,
        left: 0,
        width: '100%',
        height: '100%',
        zIndex: 1,
      } : undefined }
      { ...rest }
    >
      { /*
        Sandbox split (see also `public/nft-html-embed.html`):

        - `allow-scripts` — interactive NFT previews require JavaScript.

        - `allow-same-origin` — the **direct** document is our trusted `/nft-html-embed.html` shell
          (first-party). Same origin makes `postMessage` report a real tuple `origin` to the parent
          (opaque origins serialize as the string `"null"`), and matches how we lock the shell to
          `event.origin === window.location.origin`. MDN warns against `allow-scripts` +
          `allow-same-origin` when the framed document is **untrusted**, because script could reach
          `frameElement` and strip `sandbox`. We only use both here because the framed **top** document
          is that small shell; API-supplied HTML loads in a **nested** iframe with `allow-scripts` only
          (no `allow-same-origin`), so the NFT stays on an opaque origin and cannot read site cookies.

        We omit e.g. `allow-top-navigation`, `allow-popups`, and `allow-forms` by default to limit what
        even the shell could do if it were ever compromised.
      */ }
      <chakra.iframe
        ref={ ref }
        h="100%"
        w="100%"
        sandbox="allow-scripts allow-same-origin"
        opacity={ isLoaded ? 1 : 0 }
      />
    </Box>
  );
};

export default NftHtml;
