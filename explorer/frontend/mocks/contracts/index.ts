import type { HotContractsResponse, VerifiedContract, VerifiedContractsResponse } from 'types/api/contracts';

export const contract1: VerifiedContract = {
  address: {
    hash: '0xef490030ac0d53B70E304b6Bc5bF657dc6780bEB',
    implementations: null,
    is_contract: true,
    is_verified: null,
    name: 'MockERC20',
    private_tags: [],
    public_tags: [],
    watchlist_names: [],
    ens_domain_name: null,
  },
  coin_balance: '2346534676900000008',
  compiler_version: 'v0.8.17+commit.8df45f5f',
  has_constructor_args: false,
  language: 'solidity',
  market_cap: null,
  optimization_enabled: false,
  transactions_count: 7334224,
  verified_at: '2022-09-16T18:49:29.605179Z',
  license_type: 'mit',
};

export const contract2: VerifiedContract = {
  address: {
    hash: '0xB2218bdEbe8e90f80D04286772B0968ead666942',
    implementations: null,
    is_contract: true,
    is_verified: null,
    name: 'EternalStorageProxyWithSomeExternalLibrariesAndEvenMore',
    private_tags: [],
    public_tags: [],
    watchlist_names: [],
    ens_domain_name: null,
  },
  certified: true,
  coin_balance: '9078234570352343999',
  compiler_version: 'v0.3.1+commit.0463ea4c',
  has_constructor_args: true,
  language: 'vyper',
  market_cap: null,
  optimization_enabled: true,
  transactions_count: 440,
  verified_at: '2021-09-07T20:01:56.076979Z',
  license_type: 'bsd_3_clause',
};

export const contract3: VerifiedContract = {
  address: {
    ens_domain_name: null,
    hash: '0xf145e3A26c6706F64d95Dc8d9d45022D8b3D676B',
    implementations: [],
    is_contract: true,
    is_verified: true,
    metadata: null,
    name: 'StylusTestToken',
    private_tags: [],
    public_tags: [],
    watchlist_names: [],
  },
  certified: false,
  coin_balance: '0',
  compiler_version: 'v0.5.6',
  has_constructor_args: false,
  language: 'stylus_rust',
  license_type: 'none',
  market_cap: null,
  optimization_enabled: false,
  transactions_count: 0,
  verified_at: '2024-12-03T14:05:42.796224Z',
};

export const baseResponse: VerifiedContractsResponse = {
  items: [
    contract1,
    contract2,
    contract3,
  ],
  next_page_params: {
    items_count: '50',
    smart_contract_id: '172',
  },
};

export const hotContractsResponse: HotContractsResponse = {
  items: [
    {
      contract_address: { ...contract1.address, name: null, reputation: 'scam' },
      balance: '1000000000000000000',
      transactions_count: '1000',
      total_gas_used: '100000000',
    },
    {
      contract_address: {
        ...contract2.address,
        metadata: {
          reputation: null,
          tags: [
            { tagType: 'protocol', name: 'Goose', slug: 'goose', ordinal: 1, meta: null },
          ],
        },
      },
      balance: '420',
      transactions_count: '42',
      total_gas_used: '12343566',
    },
  ],
  next_page_params: {
    items_count: '50',
    transactions_count: '50',
    total_gas_used: '50',
    contract_address_hash: '50',
  },
};
