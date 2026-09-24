import type { UseQueryResult } from '@tanstack/react-query';
import { useQuery } from '@tanstack/react-query';

import type { TokenInstance } from 'types/api/token';

import type { DynamicRoute } from 'nextjs-routes';
import { route } from 'nextjs-routes';

import config from 'configs/app';

import type { MediaType, Size, TransportType } from './utils';
import { getPreliminaryMediaType } from './utils';

interface Params {
  data: TokenInstance;
  size: Size;
  allowedTypes?: Array<MediaType>;
  field: 'animation_url' | 'image_url';
  isEnabled: boolean;
}

interface MediaInfo {
  src: string;
  srcSet?: string;
  mediaType: MediaType;
  transport: TransportType;
}

export default function useNftMediaInfo({ data, size, allowedTypes, field, isEnabled }: Params): UseQueryResult<Array<MediaInfo> | null> {
  const url = data[field];
  const query = useQuery({
    queryKey: [ 'nft-media-info', data.id, url, size, ...(allowedTypes ? allowedTypes : []) ],
    queryFn: async() => {
      const metadataField = field === 'animation_url' ? 'animation_url' : 'image';
      const mediaType = await getMediaType(data, field);

      if (!mediaType || (allowedTypes ? !allowedTypes.includes(mediaType) : false)) {
        return null;
      }

      const cdnData = getCdnData(data, size, mediaType);
      const ipfsData = getIpfsData(data.metadata?.[metadataField], mediaType);

      return [
        cdnData,
        ipfsData,
        url ? { src: url, mediaType, transport: 'http' as const } : undefined,
      ].filter(Boolean);
    },
    enabled: isEnabled,
  });

  return query;
}

async function getMediaType(data: TokenInstance, field: Params['field']): Promise<MediaType | undefined> {
  const url = data[field];

  if (!url) {
    return;
  }

  // If the media_url is the same as the url, we can use the media_type field to determine the media type.
  if (url === data.media_url) {
    const mediaType = castMimeTypeToMediaType(data.media_type || undefined);
    if (mediaType) {
      return mediaType;
    }
  }

  // Media can be an image, video, or HTML page.
  // We pre-fetch the resources to determine their content type.
  // We must do this via Node.js due to strict CSP for connect-src.
  // To avoid overloading our server, we first check the file URL extension.
  // If it is valid, we will trust it and display the corresponding media component.

  const preliminaryType = getPreliminaryMediaType(url);

  if (preliminaryType) {
    return preliminaryType;
  }

  try {
    const mediaTypeResourceUrl = route({
      // eslint-disable-next-line max-len
      pathname: '/node-api/tokens/[hash]/instances/[id]/media-type' as DynamicRoute<'/api/tokens/[hash]/instances/[id]/media-type', { hash: string; id: string }>['pathname'],
      query: {
        hash: data.token.address_hash,
        id: data.id,
        field,
      },
    });
    const response = await fetch(mediaTypeResourceUrl);
    const payload = await response.json() as { type: MediaType | undefined };

    return payload.type;
  } catch (error) {
    return;
  }
}

function castMimeTypeToMediaType(mimeType: string | undefined): MediaType | undefined {
  if (!mimeType) {
    return;
  }

  if (mimeType.startsWith('image/')) {
    return 'image';
  }

  if (mimeType.startsWith('video/')) {
    return 'video';
  }
}

function getCdnData(data: TokenInstance, size: Size, mediaType: MediaType): MediaInfo | undefined {
  // CDN is only used for images
  if (mediaType !== 'image') {
    return;
  }

  if (!data.thumbnails) {
    return;
  }

  switch (size) {
    case 'sm': {
      return {
        src: data.thumbnails['60x60'] || data.thumbnails['250x250'] || data.thumbnails['500x500'] || data.thumbnails['original'],
        // the smallest thumbnail is already greater than sm size by two times
        // so there is no need to pass srcSet
        srcSet: undefined,
        mediaType: 'image',
        transport: 'http',
      };
    }
    case 'md': {
      const srcSet = data.thumbnails['250x250'] && data.thumbnails['500x500'] ? `${ data.thumbnails['500x500'] } 2x` : undefined;
      const src = (srcSet ? data.thumbnails['250x250'] : undefined) || data.thumbnails['500x500'] || data.thumbnails.original;

      return {
        src,
        srcSet,
        mediaType: 'image',
        transport: 'http',
      };
    }
    default: {
      if (data.thumbnails.original) {
        return {
          src: data.thumbnails.original,
          mediaType: 'image',
          transport: 'http',
        };
      }
    }
  }
}

function getIpfsData(url: unknown, mediaType: MediaType): MediaInfo | undefined {
  if (!config.UI.views.nft.verifiedFetch.isEnabled) {
    return;
  }

  // Currently we only load images via IPFS
  if (mediaType !== 'image') {
    return;
  }

  if (typeof url !== 'string') {
    return;
  }

  if (!url.includes('ipfs')) {
    return;
  }

  return {
    src: url,
    mediaType,
    transport: 'ipfs',
  };
}
