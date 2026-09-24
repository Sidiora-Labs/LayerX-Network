import type { NftMarketplaceItem } from 'types/views/nft';

import { getEnvValue, parseEnvJson } from 'configs/app/utils';

const marketplaces = (() => {
  const marketplaces = parseEnvJson<Array<NftMarketplaceItem>>(getEnvValue('NEXT_PUBLIC_VIEWS_NFT_MARKETPLACES')) || [];
  const isValid = marketplaces.every(marketplace => marketplace.collection_url || marketplace.instance_url);

  if (!isValid) {
    return [];
  }

  return marketplaces;
})();

const config = Object.freeze({
  marketplaces,
  verifiedFetch: {
    isEnabled: getEnvValue('NEXT_PUBLIC_HELIA_VERIFIED_FETCH_ENABLED') === 'false' ? false : true,
  },
});

export default config;
