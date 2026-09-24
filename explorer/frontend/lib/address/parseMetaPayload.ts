import type { AddressMetadataTag } from 'types/api/addressMetadata';
import type { AddressMetadataTagFormatted } from 'types/client/addressMetadata';

type MetaParsed = NonNullable<AddressMetadataTagFormatted['meta']>;

export default function parseMetaPayload(meta: AddressMetadataTag['meta']): AddressMetadataTagFormatted['meta'] {
  try {
    const parsedMeta = JSON.parse(meta || '');

    if (typeof parsedMeta !== 'object' || parsedMeta === null || Array.isArray(parsedMeta)) {
      throw new Error('Invalid JSON');
    }

    const result: AddressMetadataTagFormatted['meta'] = {};

    const stringFields: Array<keyof MetaParsed> = [
      'textColor',
      'bgColor',
      'tagIcon',
      'tagUrl',
      'tooltipIcon',
      'tooltipTitle',
      'tooltipDescription',
      'tooltipUrl',
      'tooltipAttribution',
      'tooltipAttributionIcon',
      'appID',
      'appMarketplaceURL',
      'appLogoURL',
      'appActionButtonText',
      'warpcastHandle',
      'data',
      'alertBgColor',
      'alertTextColor',
      'alertStatus',
      'cexDeposit',
    ];

    for (const stringField of stringFields) {
      if (stringField in parsedMeta && typeof parsedMeta[stringField as keyof typeof parsedMeta] === 'string') {
        result[stringField] = parsedMeta[stringField as keyof typeof parsedMeta];
      }
    }

    return result;
  } catch (error) {
    return null;
  }
}
