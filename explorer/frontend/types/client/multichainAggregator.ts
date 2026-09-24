import type * as bens from '@blockscout/bens-types';
import type * as multichain from '@blockscout/multichain-aggregator-types';
import type { TokenType } from 'types/api/token';

// ts-proto generates the wrong token types for AggregatedTokenInfo
// moreover, the default values of the fields (= undefined) cannot be stripped off from the generated types
// so we need to manually re-define the type to match it with core API token info type
export interface AggregatedTokenInfo extends Pick<multichain.AggregatedTokenInfo, 'chain_infos' | 'address_hash'> {
  type: TokenType;
  circulating_market_cap: string | null;
  decimals: string | null;
  holders_count: string | null;
  name: string | null;
  symbol: string | null;
  total_supply: string | null;
  exchange_rate: string | null;
  icon_url: string | null;
  reputation: null;
}

export interface AddressTokenItem extends Omit<multichain.ListAddressTokensResponse_AggregatedTokenBalanceInfo, 'token' | 'token_id'> {
  token: AggregatedTokenInfo;
  token_id: string | null;
  token_instance: null;
}

export interface AddressTokensResponse extends Omit<multichain.ListAddressTokensResponse, 'items'> {
  items: Array<AddressTokenItem>;
}

export interface TokensResponse extends Omit<multichain.ListClusterTokensResponse, 'items'> {
  items: Array<AggregatedTokenInfo>;
}

export interface AddressTokensFilter {
  type?: string;
  chain_id?: string;
  query?: string;
  include_poor_reputation_tokens?: boolean;
}

// types for quick search results
export type QuickSearchResultBlock = {
  type: 'block';
  block_number: string;
  block_hash: undefined;
  chain_id: string;
} | {
  type: 'block';
  block_number: undefined;
  block_hash: string;
  chain_id: string;
};

export interface QuickSearchResultTransaction {
  type: 'transaction';
  transaction_hash: string;
  chain_id: string;
}

export interface QuickSearchResultAddress {
  type: 'address';
  address_hash: string;
  chain_infos: Record<string, multichain.GetAddressResponse_ChainInfo>;
}

export interface QuickSearchResultToken {
  type: 'token';
  token_type: 'ERC-20' | 'ERC-721' | 'ERC-1155';
  name: string;
  symbol: string;
  address_hash: string;
  icon_url: string | null;
  is_smart_contract_verified: boolean;
  is_smart_contract_address: boolean;
  chain_id: string;
  reputation: null;
  total_supply: string | null;
  exchange_rate: string | null;
  chain_infos: Record<string, multichain.AggregatedTokenInfo_ChainInfo>;
}

export interface QuickSearchResultDomain {
  type: 'ens_domain';
  ens_info: {
    address_hash: string;
    expiry_date?: string;
    name: string;
    protocol?: bens.ProtocolInfo;
  };
  address_hash: string;
}

export type QuickSearchResultItem = QuickSearchResultBlock |
QuickSearchResultTransaction |
QuickSearchResultAddress |
QuickSearchResultToken |
QuickSearchResultDomain;

export interface ChainMetricsSorting {
  sort: 'active_accounts' | 'daily_transactions' | 'new_addresses' | 'tps';
  order: 'asc' | 'desc';
}

export type ChainMetricsSortingField = ChainMetricsSorting['sort'];

export type ChainMetricsSortingValue = `${ ChainMetricsSortingField }-${ ChainMetricsSorting['order'] }` | 'default';
