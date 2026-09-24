import type { SearchResult, SearchResultItem } from 'types/api/search';

import { ADDRESS_HASH } from './addressParams';

export const SEARCH_RESULT_ITEM: SearchResultItem = {
  address_hash: ADDRESS_HASH,
  address_url: '/address/0x3714A8C7824B22271550894f7555f0a672f97809',
  name: 'USDC',
  symbol: 'USDC',
  token_url: '/token/0x3714A8C7824B22271550894f7555f0a672f97809',
  type: 'token',
  icon_url: null,
  is_verified_via_admin_panel: false,
  is_smart_contract_verified: false,
  is_smart_contract_address: false,
  exchange_rate: '1.11',
  total_supply: null,
  token_type: 'ERC-20',
  reputation: 'ok',
};

export const SEARCH_RESULT_NEXT_PAGE_PARAMS: SearchResult['next_page_params'] = {
  address_hash: ADDRESS_HASH,
  block_hash: null,
  holders_count: 11,
  inserted_at: '2023-05-19T17:21:19.203681Z',
  item_type: 'token',
  items_count: 50,
  name: 'USDCTest',
  q: 'usd',
  transaction_hash: null,
};
