import type { AddressMetadataTagType } from 'types/api/addressMetadata';
import type { AddressMetadataTagFormatted } from 'types/client/addressMetadata';

export type EntityTagType = AddressMetadataTagType | 'custom' | 'watchlist' | 'private_tag';

export interface EntityTag extends Pick<AddressMetadataTagFormatted, 'slug' | 'name' | 'ordinal'> {
  tagType: EntityTagType;
  meta?: AddressMetadataTagFormatted['meta'];
}
