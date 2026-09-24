import type { AddressParam } from './addressParams';

export type AddressesItem = AddressParam & { transactions_count: string; coin_balance: string | null };

export type AddressesResponse = {
  items: Array<AddressesItem>;
  next_page_params: {
    fetched_coin_balance: string;
    hash: string;
    items_count: number;
  } | null;
  total_supply: string;
};

export interface AddressesMetadataSearchResult {
  items: Array<AddressesItem>;
  next_page_params: null;
}

export interface AddressesMetadataSearchFilters {
  slug: string;
  tag_type: string;
}
