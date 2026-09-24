import type { InternalTransaction, InternalTransactionsResponse } from 'types/api/internalTransaction';

export const base: InternalTransaction = {
  block_number: 29611822,
  created_contract: null,
  error: null,
  from: {
    hash: '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859',
    implementations: null,
    is_contract: true,
    is_verified: true,
    name: 'ArianeeStore',
    private_tags: [],
    public_tags: [],
    watchlist_names: [],
    ens_domain_name: null,
  },
  gas_limit: '757586',
  index: 1,
  success: true,
  timestamp: '2022-10-10T14:43:05.000000Z',
  to: {
    hash: '0x502a9C8af2441a1E276909405119FaE21F3dC421',
    implementations: null,
    is_contract: true,
    is_verified: true,
    name: 'ArianeeCreditHistory',
    private_tags: [],
    public_tags: [],
    watchlist_names: [],
    ens_domain_name: null,
  },
  transaction_hash: '0xe9e27dfeb183066e26cfe556f74b7219b08df6951e25d14003d4fc7af8bbff61',
  type: 'call',
  value: '42000000000000000000',
};

export const typeStaticCall: InternalTransaction = {
  ...base,
  type: 'staticcall',
  to: {
    ...base.to,
    name: null,
  },
  gas_limit: '63424243',
  transaction_hash: '0xe9e27dfeb183066e26cfe556f74b7219b08df6951e25d14003d4fc7af8bbff62',
};

export const withContractCreated: InternalTransaction = {
  ...base,
  type: 'delegatecall',
  to: null,
  from: {
    ...base.from,
    name: null,
  },
  created_contract: {
    hash: '0xdda21946FF3FAa027104b15BE6970CA756439F5a',
    implementations: null,
    is_contract: true,
    is_verified: null,
    name: 'Shavuha token',
    private_tags: [],
    public_tags: [],
    watchlist_names: [],
    ens_domain_name: null,
  },
  value: '1420000000000000000',
  gas_limit: '5433',
  transaction_hash: '0xe9e27dfeb183066e26cfe556f74b7219b08df6951e25d14003d4fc7af8bbff63',
};

export const baseResponse: InternalTransactionsResponse = {
  items: [
    base,
    typeStaticCall,
    withContractCreated,
  ],
  next_page_params: null,
};
