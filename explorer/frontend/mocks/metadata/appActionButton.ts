import type { AddressMetadataTagApi } from 'types/api/addressMetadata';

const appID = 'uniswap';
const appMarketplaceURL = 'https://example.com';
export const appLogoURL = 'https://localhost:3100/icon.svg';
const appActionButtonText = 'Swap';
const textColor = '#FFFFFF';
const bgColor = '#FF007A';

export const buttonWithoutStyles: AddressMetadataTagApi['meta'] = {
  appID,
  appLogoURL,
  appActionButtonText,
};

export const linkWithoutStyles: AddressMetadataTagApi['meta'] = {
  appID,
  appMarketplaceURL,
  appLogoURL,
  appActionButtonText,
};

export const buttonWithStyles: AddressMetadataTagApi['meta'] = {
  appID,
  appLogoURL,
  appActionButtonText,
  textColor,
  bgColor,
};

export const linkWithStyles: AddressMetadataTagApi['meta'] = {
  appMarketplaceURL,
  appLogoURL,
  appActionButtonText,
  textColor,
  bgColor,
};
