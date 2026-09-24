import type { HTMLChakraProps } from '@chakra-ui/react';
import type React from 'react';

export type MediaType = 'image' | 'video' | 'html';
export type TransportType = 'http' | 'ipfs';

// Currently we have only 3 sizes:
//    sm = max-width<=30px
//    md = max-width<=250px
//    original
export type Size = 'sm' | 'md' | 'original';

export type MediaElementProps<As extends React.ElementType> = HTMLChakraProps<As> & {
  src: string;
  srcSet?: string;
  transport: TransportType;
  onLoad?: () => void;
  onError?: () => void;
  onClick?: () => void;
};

// https://developer.mozilla.org/en-US/docs/Web/Media/Guides/Formats/Image_types
const IMAGE_EXTENSIONS = [
  '.jpg', 'jpeg', '.jfif', '.pjpeg', '.pjp',
  '.png', '.apng',
  '.avif',
  '.gif',
  '.svg',
  '.webp',
];

const VIDEO_EXTENSIONS = [
  '.mp4',
  '.webm',
  '.ogg',
];

export function getPreliminaryMediaType(url: string): MediaType | undefined {
  if (IMAGE_EXTENSIONS.some((ext) => url.endsWith(ext))) {
    return 'image';
  }

  if (url.startsWith('data:image')) {
    return 'image';
  }

  if (VIDEO_EXTENSIONS.some((ext) => url.endsWith(ext))) {
    return 'video';
  }
}

export const mediaStyleProps = {
  transitionProperty: 'transform',
  transitionDuration: 'normal',
  transitionTimingFunction: 'ease',
  cursor: 'pointer',
  _hover: {
    base: {},
    lg: {
      transform: 'scale(1.2)',
    },
  },
};

export const videoPlayProps = {
  disablePictureInPicture: true,
  loop: true,
  muted: true,
  playsInline: true,
};
