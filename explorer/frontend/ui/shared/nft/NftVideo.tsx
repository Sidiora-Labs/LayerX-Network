import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { TokenInstance } from 'types/api/token';

import useLoadImageViaIpfs from './useLoadImageViaIpfs';
import useNftMediaInfo from './useNftMediaInfo';
import type { MediaElementProps, Size } from './utils';
import { videoPlayProps } from './utils';

interface Props extends MediaElementProps<'video'> {
  instance: TokenInstance;
  autoPlay?: boolean;
  size?: Size;
}

const POSTER_ALLOWED_TYPES = [ 'image' as const ];

const NftVideo = ({ src, transport, instance, autoPlay = true, onLoad, size = 'original', onError, onClick, ...rest }: Props) => {
  const ref = React.useRef<HTMLVideoElement>(null);

  const mediaInfoQuery = useNftMediaInfo({ data: instance, size, field: 'image_url', isEnabled: true, allowedTypes: POSTER_ALLOWED_TYPES });
  const loadImageViaIpfs = useLoadImageViaIpfs();

  const handleMouseEnter = React.useCallback(() => {
    !autoPlay && ref.current?.play();
  }, [ autoPlay ]);

  const handleMouseLeave = React.useCallback(() => {
    !autoPlay && ref.current?.pause();
  }, [ autoPlay ]);

  const loadViaHttp = React.useCallback(async() => {
    if (!ref.current) {
      return;
    }

    ref.current.src = src;
    onLoad && (ref.current.oncanplaythrough = onLoad);
    onError && (ref.current.onerror = onError);
  }, [ src, onLoad, onError ]);

  React.useEffect(() => {
    switch (transport) {
      case 'ipfs': {
        // Currently we don't support IPFS video loading
        onError?.();
        break;
      }
      case 'http':
        loadViaHttp();
        break;
    }
  }, [ loadViaHttp, onError, transport ]);

  const loadPosterViaHttp = React.useCallback(async(src: string) => {
    if (!ref.current || !ref.current.poster) {
      return;
    }

    const poster = new Image();
    poster.src = src;
    // we want to call onLoad right after the poster is loaded
    // otherwise, the skeleton will be shown underneath the element until the video is loaded
    onLoad && (poster.onload = onLoad);

    ref.current.poster = poster.src;
  }, [ onLoad ]);

  const loadPosterViaIpfs = React.useCallback((url: string) => {
    loadImageViaIpfs(url)
      .then((src) => {
        if (src && ref.current) {
          ref.current.poster = src;
          onLoad?.();
        }
      })
      .catch(onError);
  }, [ loadImageViaIpfs, onError, onLoad ]);

  React.useEffect(() => {
    if (autoPlay) {
      return;
    }

    if (!mediaInfoQuery.isPending && mediaInfoQuery.data) {
      const mediaInfo = mediaInfoQuery.data[0];
      switch (mediaInfo.transport) {
        case 'ipfs':
          loadPosterViaIpfs(mediaInfo.src);
          break;
        case 'http':
          loadPosterViaHttp(mediaInfo.src);
          break;
      }
    }

  }, [ autoPlay, loadPosterViaHttp, loadPosterViaIpfs, mediaInfoQuery.data, mediaInfoQuery.isPending ]);

  return (
    <chakra.video
      ref={ ref }
      { ...videoPlayProps }
      autoPlay={ autoPlay }
      onClick={ onClick }
      onMouseEnter={ handleMouseEnter }
      onMouseLeave={ handleMouseLeave }
      { ...rest }
    />
  );
};

export default React.memo(NftVideo);
