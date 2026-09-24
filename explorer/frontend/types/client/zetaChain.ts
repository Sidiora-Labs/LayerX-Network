import type { AdvancedFilterAge } from 'types/api/advancedFilter';

import type { ExternalChain } from '../externalChains';

export const ZETA_CHAIN_CCTX_STATUS_REDUCED_FILTERS = [ 'Success', 'Pending', 'Failed' ] as const;
export type StatusReducedFilters = typeof ZETA_CHAIN_CCTX_STATUS_REDUCED_FILTERS[number];

export const ZETA_CHAIN_CCTX_COIN_TYPE_FILTER = 'Zeta' as const;
export type CoinTypeFilter = typeof ZETA_CHAIN_CCTX_COIN_TYPE_FILTER;

export type ZetaChainCCTXFilterParams = {
  start_timestamp?: string;
  end_timestamp?: string;
  age?: AdvancedFilterAge | ''; /* frontend only */
  status_reduced?: Array<StatusReducedFilters> | StatusReducedFilters;
  sender_address?: Array<string> | string;
  receiver_address?: Array<string> | string;
  source_chain_id?: Array<string> | string;
  target_chain_id?: Array<string> | string;
  token_symbol?: Array<string> | string;
  coin_type?: Array<CoinTypeFilter> | CoinTypeFilter;
  hash?: string;
};

export interface ZetaChainChainsConfigEnv {
  chain_id: number;
  chain_name: string | null;
  chain_logo?: string | null;
  instance_url?: string;
  address_url_template?: string;
  tx_url_template?: string;
}

export type ZetaChainExternalChainConfig = ExternalChain | {
  id: string;
  name: string;
  logo: string | undefined;
  address_url_template?: string;
  tx_url_template?: string;
};
