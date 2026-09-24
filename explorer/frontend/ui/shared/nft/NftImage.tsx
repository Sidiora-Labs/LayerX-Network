import React from 'react';

import { Image } from 'toolkit/chakra/image';

import useLoadImageViaIpfs from './useLoadImageViaIpfs';
import type { MediaElementProps } from './utils';

interface Props extends MediaElementProps<'img'> {}

const NftImage = ({ src, srcSet, onLoad, onError, transport, onClick, ...rest }: Props) => {
  const ref = React.useRef<HTMLImageElement>(null);
  const [ isLoaded, setIsLoaded ] = React.useState(false);

  const handleLoad = React.useCallback(() => {
    setIsLoaded(true);
    onLoad?.();
  }, [ onLoad ]);

  const loadImageViaIpfs = useLoadImageViaIpfs();

  const loadViaHttp = React.useCallback(async() => {
    if (!ref.current) {
      return;
    }

    ref.current.src = src;
    srcSet && (ref.current.srcset = srcSet);
    ref.current.onload = handleLoad;
    onError && (ref.current.onerror = onError);
  }, [ src, srcSet, handleLoad, onError ]);

  const loadViaIpfs = React.useCallback(() => {
    loadImageViaIpfs(src)
      .then((src) => {
        if (src && ref.current) {
          ref.current.src = src;
          handleLoad();
        }
      })
      .catch(onError);
  }, [ handleLoad, loadImageViaIpfs, onError, src ]);

  React.useEffect(() => {
    switch (transport) {
      case 'ipfs':
        loadViaIpfs();
        break;
      case 'http':
        loadViaHttp();
        break;
    }
  }, [ loadViaHttp, loadViaIpfs, transport ]);

  return (
    <Image
      ref={ ref }
      w="100%"
      h="100%"
      opacity={ isLoaded ? 1 : 0 }
      alt="Token instance image"
      onClick={ onClick }
      { ...rest }
    />
  );
};

export default React.memo(NftImage);
