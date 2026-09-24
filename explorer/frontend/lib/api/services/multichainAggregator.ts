import type { ApiResource } from '../types';
import type * as multichain from '@blockscout/multichain-aggregator-types';
import type { AddressTokensFilter, AddressTokensResponse, TokensResponse } from 'types/client/multichainAggregator';

export const MULTICHAIN_AGGREGATOR_API_RESOURCES = {
  address: {
    path: '/addresses/:hash',
    pathParams: [ 'hash' as const ],
  },
  address_tokens: {
    path: '/addresses/:hash/tokens',
    pathParams: [ 'hash' as const ],
    paginated: true,
    filterFields: [ 'chain_id' as const, 'type' as const, 'query' as const ],
  },
  address_domains: {
    path: '/addresses/:hash/domains',
    pathParams: [ 'hash' as const ],
    paginated: true,
  },
  address_portfolio: {
    path: '/addresses/:hash/portfolio',
    pathParams: [ 'hash' as const ],
    filterFields: [ 'chain_id' as const, 'query' as const ],
  },
  tokens: {
    path: '/tokens',
    filterFields: [ 'chain_id' as const, 'type' as const, 'query' as const ],
    paginated: true,
  },
  domain_protocols: {
    path: '/domain-protocols',
  },
  quick_search: {
    path: '/search\\:quick',
    filterFields: [ 'q' as const ],
  },
  search_check_redirect: {
    path: '/search\\:check-redirect',
  },
  search_addresses: {
    path: '/search/addresses',
    filterFields: [ 'q' as const, 'chain_id' as const ],
    paginated: true,
  },
  search_blocks: {
    path: '/search/blocks',
    filterFields: [ 'q' as const, 'chain_id' as const ],
    paginated: true,
  },
  search_block_numbers: {
    path: '/search/block-numbers',
    filterFields: [ 'q' as const, 'chain_id' as const ],
    paginated: true,
  },
  search_transactions: {
    path: '/search/transactions',
    filterFields: [ 'q' as const, 'chain_id' as const ],
    paginated: true,
  },
  search_tokens: {
    path: '/search/tokens',
    filterFields: [ 'q' as const, 'chain_id' as const ],
    paginated: true,
  },
  search_nfts: {
    path: '/search/nfts',
    filterFields: [ 'q' as const, 'chain_id' as const ],
    paginated: true,
  },
  search_domains: {
    path: '/search/domains',
    filterFields: [ 'q' as const, 'chain_id' as const ],
    paginated: true,
  },
  chain_metrics: {
    path: '/chain-metrics',
  },
} satisfies Record<string, ApiResource>;

export type MultichainAggregatorApiResourceName = `multichainAggregator:${ keyof typeof MULTICHAIN_AGGREGATOR_API_RESOURCES }`;

/* eslint-disable @stylistic/indent */
export type MultichainAggregatorApiResourcePayload<R extends MultichainAggregatorApiResourceName> =
R extends 'multichainAggregator:address' ? multichain.GetAddressResponse :
R extends 'multichainAggregator:address_tokens' ? AddressTokensResponse :
R extends 'multichainAggregator:address_portfolio' ? multichain.GetAddressPortfolioResponse :
R extends 'multichainAggregator:address_domains' ? multichain.LookupAddressDomainsResponse :
R extends 'multichainAggregator:tokens' ? TokensResponse :
R extends 'multichainAggregator:domain_protocols' ? multichain.ListDomainProtocolsResponse :
R extends 'multichainAggregator:quick_search' ? multichain.ClusterQuickSearchResponse :
R extends 'multichainAggregator:search_check_redirect' ? multichain.CheckRedirectResponse :
R extends 'multichainAggregator:search_addresses' ? multichain.SearchAddressesResponse :
R extends 'multichainAggregator:search_blocks' ? multichain.SearchBlocksResponse :
R extends 'multichainAggregator:search_block_numbers' ? multichain.SearchBlockNumbersResponse :
R extends 'multichainAggregator:search_transactions' ? multichain.SearchTransactionsResponse :
R extends 'multichainAggregator:search_tokens' ? multichain.SearchTokensResponse :
R extends 'multichainAggregator:search_nfts' ? multichain.SearchNftsResponse :
R extends 'multichainAggregator:search_domains' ? multichain.SearchDomainsResponse :
R extends 'multichainAggregator:chain_metrics' ? multichain.ListChainMetricsResponse :
never;
/* eslint-enable @stylistic/indent */

/* eslint-disable @stylistic/indent */
export type MultichainAggregatorApiPaginationFilters<R extends MultichainAggregatorApiResourceName> =
R extends 'multichainAggregator:address_tokens' ? AddressTokensFilter :
R extends 'multichainAggregator:tokens' ? Partial<multichain.ListClusterTokensRequest> :
never;
/* eslint-enable @stylistic/indent */
