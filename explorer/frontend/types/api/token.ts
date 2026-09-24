import type { TokenInfoApplication } from './account';
import type { AddressParam } from './addressParams';

export type NFTTokenType = 'ERC-721' | 'ERC-1155' | 'ERC-404';
// token type can come from the environment config, so it can be any string
export type TokenType = string;

export type TokenReputation = 'ok' | 'scam';

export interface TokenInfo {
  address_hash: string;
  type: TokenType;
  symbol: string | null;
  name: string | null;
  decimals: string | null;
  holders_count: string | null;
  exchange_rate: string | null;
  total_supply: string | null;
  icon_url: string | null;
  circulating_market_cap: string | null;
  reputation: TokenReputation | null;
  // bridged token fields
  is_bridged?: boolean | null;
  bridge_type?: string | null;
  origin_chain_id?: string | null;
  foreign_address?: string | null;
  filecoin_robust_address?: string | null;
  zilliqa?: { zrc2_address_hash?: string };
}

export interface TokenCounters {
  token_holders_count: string;
  transfers_count: string;
}

export interface TokenHolders {
  items: Array<TokenHolder>;
  next_page_params: TokenHoldersPagination | null;
}

export type TokenHolder = TokenHolderERC20ERC721 | TokenHolderERC1155;

export type TokenHolderBase = {
  address: AddressParam;
  value: string;
};

export type TokenHolderERC20ERC721 = TokenHolderBase;

export type TokenHolderERC1155 = TokenHolderBase & {
  token_id: string;
};

export type TokenHoldersPagination = {
  items_count: number;
  value: string;
};

export type ThumbnailSize = '60x60' | '250x250' | '500x500' | 'original';

export interface TokenInstance {
  is_unique: boolean;
  id: string;
  holder_address_hash: string | null;
  image_url: string | null;
  animation_url: string | null;
  media_url?: string | null;
  media_type?: string | null;
  external_app_url: string | null;
  metadata: Record<string, unknown> | null;
  owner: AddressParam | null;
  thumbnails: ({ original: string } & Partial<Record<Exclude<ThumbnailSize, 'original'>, string>>) | null;
  token: TokenInfo;
}

export interface TokenInstanceMetadataSocketMessage {
  token_id: number;
  fetched_metadata: TokenInstance['metadata'];
}

export interface TokenInstanceTransfersCount {
  transfers_count: number;
}

export interface TokenInventoryResponse {
  items: Array<TokenInstance>;
  next_page_params: TokenInventoryPagination | null;
}

export type TokenInventoryPagination = {
  unique_token: number;
};

export type TokenVerifiedInfo = Omit<TokenInfoApplication, 'id' | 'status'>;

export type TokenInventoryFilters = {
  holder_address_hash?: string;
};
