import type { AddressMetadataTagApi } from 'types/api/addressMetadata';

export interface AddressMetadataInfoFormatted {
  addresses: Record<string, {
    tags: Array<AddressMetadataTagFormatted>;
    reputation: number | null;
  }>;
}

export type AddressMetadataTagFormatted = AddressMetadataTagApi;
