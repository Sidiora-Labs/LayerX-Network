import type { BoxProps, ImageProps as ChakraImageProps } from '@chakra-ui/react';
import { Image as ChakraImage } from '@chakra-ui/react';
import React from 'react';

import { Skeleton } from './skeleton';

export interface ImageProps extends ChakraImageProps {
  fallback?: React.ReactNode;
  // for the case where the image dimensions are not known before the image is loaded
  skeletonWidth?: BoxProps['width'];
  skeletonHeight?: BoxProps['height'];
}

export const Image = React.forwardRef<HTMLImageElement, ImageProps>(
  function Image(props, ref) {
    const { fallback, src, onLoad, onError, skeletonWidth, skeletonHeight, alt, ...rest } = props;

    const [ loading, setLoading ] = React.useState(true);
    const [ error, setError ] = React.useState(false);

    const handleLoadError = React.useCallback((event: React.SyntheticEvent<HTMLImageElement>) => {
      setError(true);
      setLoading(false);
      onError?.(event);
    }, [ onError ]);

    const handleLoadSuccess = React.useCallback((event: React.SyntheticEvent<HTMLImageElement>) => {
      setLoading(false);
      onLoad?.(event);
    }, [ onLoad ]);

    if (!src && fallback) {
      if (React.isValidElement(fallback)) {
        return React.cloneElement(fallback, rest);
      }
      return fallback;
    }

    if (error) {
      if (React.isValidElement(fallback)) {
        return React.cloneElement(fallback, rest);
      }
      return fallback;
    }

    const skeletonProps: BoxProps = {
      ...rest as BoxProps,
      ...(skeletonWidth !== undefined && { width: skeletonWidth }),
      ...(skeletonHeight !== undefined && { height: skeletonHeight }),
    };

    return (
      <>
        { loading && <Skeleton loading { ...skeletonProps }/> }
        <ChakraImage
          ref={ ref }
          src={ src }
          alt={ alt }
          onError={ handleLoadError }
          onLoad={ handleLoadSuccess }
          { ...rest }
          display={ loading ? 'none' : rest.display || 'block' }
        />
      </>
    );
  },
);
