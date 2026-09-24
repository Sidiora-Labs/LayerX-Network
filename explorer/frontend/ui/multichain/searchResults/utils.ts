import type { ReturnType } from 'lib/api/useApiInfiniteQuery';

export interface SearchQueries {
  addresses: ReturnType<'multichainAggregator:search_addresses'>;
  tokens: ReturnType<'multichainAggregator:search_tokens'>;
  blockNumbers: ReturnType<'multichainAggregator:search_block_numbers'>;
  blocks: ReturnType<'multichainAggregator:search_blocks'>;
  transactions: ReturnType<'multichainAggregator:search_transactions'>;
  nfts: ReturnType<'multichainAggregator:search_nfts'>;
  domains: ReturnType<'multichainAggregator:search_domains'>;
}

export type QueryType = keyof SearchQueries;

export const SEARCH_TABS_NAMES: Record<QueryType, string> = {
  tokens: 'Tokens (ERC-20)',
  nfts: 'NFTs (ERC-721 & 1155)',
  addresses: 'Addresses',
  blockNumbers: 'Block numbers',
  blocks: 'Blocks',
  transactions: 'Transactions',
  domains: 'Names',
};

export const SEARCH_TABS_IDS: Record<QueryType, string> = {
  addresses: 'addresses',
  tokens: 'tokens',
  blockNumbers: 'block_numbers',
  blocks: 'blocks',
  transactions: 'transactions',
  nfts: 'nfts',
  domains: 'names',
};
