import type { HTMLChakraProps } from '@chakra-ui/react';
import { AspectRatio, Box } from '@chakra-ui/react';
import React from 'react';
import { useInView } from 'react-intersection-observer';

import type { TokenInstance } from 'types/api/token';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';

import NftFallback from './NftFallback';
import NftHtml from './NftHtml';
import NftImage from './NftImage';
import NftMediaFullscreenModal from './NftMediaFullscreenModal';
import NftVideo from './NftVideo';
import useNftMediaInfo from './useNftMediaInfo';
import type { MediaType, Size } from './utils';
import { mediaStyleProps } from './utils';

interface Props extends Omit<HTMLChakraProps<'div'>, 'size'> {
  data: TokenInstance;
  size?: Size;
  allowedTypes?: Array<MediaType>;
  isLoading?: boolean;
  withFullscreen?: boolean;
  autoplayVideo?: boolean;
  fallback?: React.ReactNode;
}

const NftMedia = ({ data, size = 'original', allowedTypes, isLoading, withFullscreen, autoplayVideo, fallback, ...rest }: Props) => {
  const [ isMediaLoading, setIsMediaLoading ] = React.useState(true);
  const [ isMediaLoadingError, setIsMediaLoadingError ] = React.useState(false);
  const [ mediaInfoIndex, setMediaInfoIndex ] = React.useState(0);
  const [ mediaInfoField, setMediaInfoField ] = React.useState<'animation_url' | 'image_url'>('animation_url');

  const { ref, inView } = useInView({ triggerOnce: true });

  const mediaInfoQuery = useNftMediaInfo({ data, size, allowedTypes, field: mediaInfoField, isEnabled: !isLoading && inView });

  React.useEffect(() => {
    if (!mediaInfoQuery.isPending && !mediaInfoQuery.data) {
      if (mediaInfoField === 'animation_url') {
        setMediaInfoField('image_url');
      } else {
        setIsMediaLoadingError(true);
        setIsMediaLoading(false);
      }
    }
  }, [ mediaInfoQuery.isPending, mediaInfoQuery.data, mediaInfoField ]);

  const handleMediaLoaded = React.useCallback(() => {
    setIsMediaLoading(false);
  }, []);

  const handleMediaLoadError = React.useCallback(() => {
    if (mediaInfoQuery.data) {
      if (mediaInfoIndex < mediaInfoQuery.data.length - 1) {
        setMediaInfoIndex(mediaInfoIndex + 1);
        return;
      } else if (mediaInfoField === 'animation_url') {
        setMediaInfoField('image_url');
        setMediaInfoIndex(0);
        return;
      }
    }

    setIsMediaLoading(false);
    setIsMediaLoadingError(true);
  }, [ mediaInfoField, mediaInfoIndex, mediaInfoQuery.data ]);

  const { open, onOpen, onOpenChange } = useDisclosure();

  const content = (() => {
    if (isLoading) {
      return null;
    }

    if (isMediaLoadingError) {
      const styleProps = withFullscreen ? {} : mediaStyleProps;
      return fallback ?? <NftFallback { ...styleProps }/>;
    }

    const mediaInfo = mediaInfoQuery.data?.[mediaInfoIndex];
    const props = {
      onLoad: handleMediaLoaded,
      onError: handleMediaLoadError,
      ...(withFullscreen ? { onClick: onOpen } : {}),
      ...(size !== 'sm' ? mediaStyleProps : {}),
    };

    switch (mediaInfo?.mediaType) {
      case 'video': {
        return <NftVideo { ...props } src={ mediaInfo.src } transport={ mediaInfo.transport } autoPlay={ autoplayVideo } instance={ data }/>;
      }
      case 'html':
        return <NftHtml { ...props } src={ mediaInfo.src } transport={ mediaInfo.transport }/>;
      case 'image': {
        return <NftImage { ...props } src={ mediaInfo.src } srcSet={ mediaInfo.srcSet } transport={ mediaInfo.transport }/>;
      }
      default:
        return null;
    }
  })();

  return (
    <>
      <AspectRatio
        ref={ ref }
        ratio={ 1 / 1 }
        overflow="hidden"
        borderRadius="md"
        isolation="isolate"
        { ...rest }
      >
        <>
          <Box
            css={{
              '& > img, & > video': {
                objectFit: 'contain',
              },
            }}
          >
            { content }
          </Box>
          { isMediaLoading && <Skeleton loading position="absolute" left={ 0 } top={ 0 } w="100%" h="100%" zIndex="1"/> }
        </>
      </AspectRatio>
      { open && (
        <NftMediaFullscreenModal open={ open } onOpenChange={ onOpenChange } data={ data } allowedTypes={ allowedTypes } field={ mediaInfoField }/>
      ) }
    </>
  );
};

export default React.memo(NftMedia);
