import type { CctxListItem } from '@blockscout/zetachain-cctx-types';
import { getFeaturePayload } from 'configs/app/features/types';
import type { TokenType } from 'types/api/token';
import type { MarketplaceApp } from 'types/client/marketplace';
import type { QuickSearchResultItem } from 'types/client/search';

import config from 'configs/app';
import { isConfidentialTokenType } from 'lib/token/tokenTypes';

const nameServicesFeature = config.features.nameServices;

const dappEntityName = getFeaturePayload(config.features.marketplace)?.titles.entity_name ?? '';

export type ApiCategory =
  'token' |
  'nft' |
  'address' |
  'public_tag' |
  'transaction' |
  'block' |
  'user_operation' |
  'blob' |
  'domain' |
  'cluster' |
  'tac_operation' |
  'confidential_token';
export type Category = ApiCategory | 'app' | 'zetaChainCCTX';

export type ItemsCategoriesMap =
Record<ApiCategory, Array<QuickSearchResultItem>> &
Record<'app', Array<MarketplaceApp>> &
Record<'zetaChainCCTX', Array<CctxListItem>>;

export type SearchResultAppItem = {
  type: 'app';
  app: MarketplaceApp;
};

const hasConfidentialTokenType = config.chain.additionalTokenTypes.some((item) => isConfidentialTokenType(item.id as TokenType));

export const searchCategories: Array<{ id: Category; title: string; tabTitle: string }> = [
  { id: 'token', title: `Tokens (${ config.chain.tokenStandard }-20)`, tabTitle: 'Tokens' },
  { id: 'nft', title: `NFTs (${ config.chain.tokenStandard }-721 & 1155)`, tabTitle: 'NFTs' },
  ...(hasConfidentialTokenType ? [
    { id: 'confidential_token' as const, title: `Confidential Tokens (${ config.chain.tokenStandard }-7984)`, tabTitle: 'Confidential Tokens' },
  ] : []),
  { id: 'address', title: 'Addresses', tabTitle: 'Addresses' },
  { id: 'public_tag', title: 'Public tags', tabTitle: 'Public tags' },
  { id: 'transaction', title: 'Transactions', tabTitle: 'Transactions' },
  { id: 'block', title: 'Blocks', tabTitle: 'Blocks' },
  { id: 'tac_operation', title: 'Operations', tabTitle: 'Operations' },
  { id: 'zetaChainCCTX', title: 'CCTXs', tabTitle: 'CCTXs' },
];

if (config.features.userOps.isEnabled) {
  searchCategories.push({ id: 'user_operation', title: 'User operations', tabTitle: 'User ops' });
}

if (config.features.dataAvailability.isEnabled) {
  searchCategories.push({ id: 'blob', title: 'Blobs', tabTitle: 'Blobs' });
}

if (config.features.marketplace.isEnabled) {
  searchCategories.unshift({ id: 'app', title: `${ dappEntityName }s`, tabTitle: `${ dappEntityName }s` });
}

if ((nameServicesFeature.isEnabled && nameServicesFeature.ens.isEnabled) || config.features.multichain.isEnabled) {
  searchCategories.unshift({ id: 'domain', title: 'Names', tabTitle: 'Names' });
}

if (nameServicesFeature.isEnabled && nameServicesFeature.clusters.isEnabled) {
  searchCategories.unshift({ id: 'cluster', title: 'Cluster Name', tabTitle: 'Cluster' });
}

export const searchItemTitles: Record<Category, { itemTitle: string; itemTitleShort: string }> = {
  app: { itemTitle: dappEntityName, itemTitleShort: dappEntityName },
  domain: { itemTitle: 'Name', itemTitleShort: 'Name' },
  cluster: { itemTitle: 'Cluster', itemTitleShort: 'Cluster' },
  token: { itemTitle: 'Token', itemTitleShort: 'Token' },
  nft: { itemTitle: 'NFT', itemTitleShort: 'NFT' },
  confidential_token: { itemTitle: 'Confidential Token', itemTitleShort: 'Conf. Token' },
  address: { itemTitle: 'Address', itemTitleShort: 'Address' },
  public_tag: { itemTitle: 'Public tag', itemTitleShort: 'Tag' },
  transaction: { itemTitle: 'Transaction', itemTitleShort: 'Txn' },
  block: { itemTitle: 'Block', itemTitleShort: 'Block' },
  user_operation: { itemTitle: 'User operation', itemTitleShort: 'User op' },
  blob: { itemTitle: 'Blob', itemTitleShort: 'Blob' },
  tac_operation: { itemTitle: 'Operations', itemTitleShort: 'Operations' },
  zetaChainCCTX: { itemTitle: 'CCTX', itemTitleShort: 'CCTX' },
};

export function getItemCategory(item: QuickSearchResultItem | SearchResultAppItem): Category | undefined {
  switch (item.type) {
    case 'address':
    case 'contract':
    case 'metadata_tag': {
      return 'address';
    }
    case 'token': {
      if (item.token_type === 'ERC-20') {
        return 'token';
      }
      if (hasConfidentialTokenType && isConfidentialTokenType(item.token_type as TokenType)) {
        return 'confidential_token';
      }
      return 'nft';
    }
    case 'block': {
      return 'block';
    }
    case 'label': {
      return 'public_tag';
    }
    case 'transaction': {
      return 'transaction';
    }
    case 'app': {
      return 'app';
    }
    case 'user_operation': {
      return 'user_operation';
    }
    case 'blob': {
      return 'blob';
    }
    case 'ens_domain': {
      return 'domain';
    }
    case 'cluster': {
      return 'cluster';
    }
    case 'tac_operation': {
      return 'tac_operation';
    }
    case 'zetaChainCCTX': {
      return 'zetaChainCCTX';
    }
  }
}
