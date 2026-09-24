export interface AddressMetadataInfo {
  addresses: Record<string, {
    tags: Array<AddressMetadataTag>;
    reputation: number | null;
  }>;
}

export type AddressMetadataTagType = 'name' | 'generic' | 'classifier' | 'information' | 'note' | 'protocol';

// Response model from Metadata microservice API
export interface AddressMetadataTag {
  slug: string;
  name: string;
  tagType: AddressMetadataTagType;
  ordinal: number;
  meta: string | null;
}

// Response model from Blockscout API with parsed meta field
export interface AddressMetadataTagApi extends Omit<AddressMetadataTag, 'meta'> {
  meta: {
    textColor?: string;
    bgColor?: string;
    tagIcon?: string;
    tagUrl?: string;
    tooltipIcon?: string;
    tooltipTitle?: string;
    tooltipDescription?: string;
    tooltipUrl?: string;
    tooltipAttribution?: string;
    tooltipAttributionIcon?: string;
    appID?: string;
    appMarketplaceURL?: string;
    appLogoURL?: string;
    appActionButtonText?: string;
    warpcastHandle?: string;
    data?: string;
    alertBgColor?: string;
    alertTextColor?: string;
    alertStatus?: string;
    cexDeposit?: string;
  } | null;
}

// TAG SUBMISSION

export interface PublicTagType {
  id: string;
  type: AddressMetadataTagType;
  description: string;
}

export interface PublicTagTypesResponse {
  tagTypes: Array<PublicTagType>;
}
