import type { ApiResource } from '../types';
import type { AddressMetadataInfo, PublicTagTypesResponse } from 'types/api/addressMetadata';

export const METADATA_API_RESOURCES = {
  info: {
    path: '/api/v1/metadata',
  },
  tags_search: {
    path: '/api/v1/tags:search',
  },
  public_tag_types: {
    path: '/api/v1/public-tag-types',
  },
  address_submit: {
    path: '/api/v1/addresses\\:submit',
  },
} satisfies Record<string, ApiResource>;

export type MetadataApiResourceName = `metadata:${ keyof typeof METADATA_API_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type MetadataApiResourcePayload<R extends MetadataApiResourceName> =
R extends 'metadata:info' ? AddressMetadataInfo :
R extends 'metadata:public_tag_types' ? PublicTagTypesResponse :
never;
/* eslint-enable @stylistic/indent */
