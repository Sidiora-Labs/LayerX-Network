import type {
  SearchResultToken,
  SearchResultBlock,
  SearchResultAddressOrContract,
  SearchResultTx,
  SearchResultLabel,
  SearchResult,
  SearchResultUserOp,
  SearchResultBlob,
  SearchResultDomain,
  SearchResultMetadataTag,
  SearchResultTacOperation,
} from 'types/api/search';

import * as tacOperationMock from 'mocks/operations/tac';

export const token1: SearchResultToken = {
  address_hash: '0x377c5F2B300B25a534d4639177873b7fEAA56d4B',
  address_url: '/address/0x377c5F2B300B25a534d4639177873b7fEAA56d4B',
  name: 'Toms NFT',
  symbol: 'TNT',
  token_url: '/token/0x377c5F2B300B25a534d4639177873b7fEAA56d4B',
  type: 'token' as const,
  icon_url: 'http://localhost:3000/token-icon.png',
  token_type: 'ERC-721',
  total_supply: '10000001',
  exchange_rate: null,
  is_verified_via_admin_panel: true,
  is_smart_contract_verified: true,
  is_smart_contract_address: true,
  reputation: 'ok',
};

export const token2: SearchResultToken = {
  address_hash: '0xC35Cc7223B0175245E9964f2E3119c261E8e21F9',
  address_url: '/address/0xC35Cc7223B0175245E9964f2E3119c261E8e21F9',
  name: 'TomToken',
  symbol: 'pdE1B',
  token_url: '/token/0xC35Cc7223B0175245E9964f2E3119c261E8e21F9',
  type: 'token' as const,
  icon_url: null,
  token_type: 'ERC-20',
  total_supply: '10000001',
  exchange_rate: '1.11',
  is_verified_via_admin_panel: false,
  is_smart_contract_verified: false,
  is_smart_contract_address: false,
  reputation: 'ok',
};

export const block1: SearchResultBlock = {
  block_hash: '0x1af31d7535dded06bab9a88eb40ee2f8d0529a60ab3b8a7be2ba69b008cacbd1',
  block_number: 8198536,
  type: 'block' as const,
  timestamp: '2022-12-11T17:55:20Z',
  url: '/block/0x1af31d7535dded06bab9a88eb40ee2f8d0529a60ab3b8a7be2ba69b008cacbd1',
};

export const block2: SearchResultBlock = {
  block_hash: '0x1af31d7535dded06bab9a88eb40ee2f8d0529a60ab3b8a7be2ba69b008cacbd2',
  block_number: 8198536,
  block_type: 'reorg',
  type: 'block' as const,
  timestamp: '2022-12-11T18:55:20Z',
  url: '/block/0x1af31d7535dded06bab9a88eb40ee2f8d0529a60ab3b8a7be2ba69b008cacbd2',
};

export const block3: SearchResultBlock = {
  block_hash: '0x1af31d7535dded06bab9a88eb40ee2f8d0529a60ab3b8a7be2ba69b008cacbd3',
  block_number: 8198536,
  block_type: 'uncle',
  type: 'block' as const,
  timestamp: '2022-12-11T18:11:11Z',
  url: '/block/0x1af31d7535dded06bab9a88eb40ee2f8d0529a60ab3b8a7be2ba69b008cacbd3',
};

export const address1: SearchResultAddressOrContract = {
  address_hash: '0xb64a30399f7F6b0C154c2E7Af0a3ec7B0A5b131a',
  name: null,
  type: 'address' as const,
  is_smart_contract_verified: false,
  is_smart_contract_address: false,
  url: '/address/0xb64a30399f7F6b0C154c2E7Af0a3ec7B0A5b131a',
};

export const address2: SearchResultAddressOrContract = {
  address_hash: '0xb64a30399f7F6b0C154c2E7Af0a3ec7B0A5b131b',
  name: null,
  type: 'address' as const,
  is_smart_contract_verified: false,
  is_smart_contract_address: false,
  url: '/address/0xb64a30399f7F6b0C154c2E7Af0a3ec7B0A5b131b',
  ens_info: {
    address_hash: '0x1234567890123456789012345678901234567890',
    expiry_date: '2022-12-11T17:55:20Z',
    name: 'utko.eth',
    names_count: 1,
  },
};

export const contract1: SearchResultAddressOrContract = {
  address_hash: '0xb64a30399f7F6b0C154c2E7Af0a3ec7B0A5b131a',
  name: 'Unknown contract in this network',
  type: 'contract' as const,
  is_smart_contract_verified: true,
  is_smart_contract_address: true,
  url: '/address/0xb64a30399f7F6b0C154c2E7Af0a3ec7B0A5b131a',
};

export const contract2: SearchResultAddressOrContract = {
  address_hash: '0xb64a30399f7F6b0C154c2E7Af0a3ec7B0A5b131a',
  name: 'Super utko',
  type: 'contract' as const,
  is_smart_contract_verified: true,
  certified: true,
  is_smart_contract_address: true,
  url: '/address/0xb64a30399f7F6b0C154c2E7Af0a3ec7B0A5b131a',
};

export const label1: SearchResultLabel = {
  address_hash: '0xb64a30399f7F6b0C154c2E7Af0a3ec7B0A5b131a',
  name: 'utko',
  type: 'label' as const,
  is_smart_contract_verified: true,
  is_smart_contract_address: true,
  url: '/address/0xb64a30399f7F6b0C154c2E7Af0a3ec7B0A5b131a',
};

export const tx1: SearchResultTx = {
  transaction_hash: '0x349d4025d03c6faec117ee10ac0bce7c7a805dd2cbff7a9f101304d9a8a525dd',
  type: 'transaction' as const,
  timestamp: '2022-12-11T17:55:20Z',
  url: '/tx/0x349d4025d03c6faec117ee10ac0bce7c7a805dd2cbff7a9f101304d9a8a525dd',
};

export const userOp1: SearchResultUserOp = {
  timestamp: '2024-01-11T14:15:48.000000Z',
  type: 'user_operation',
  user_operation_hash: '0xcb560d77b0f3af074fa05c1e5c691bcdfe457e630062b5907e9e71fc74b2ec61',
  url: '/op/0xcb560d77b0f3af074fa05c1e5c691bcdfe457e630062b5907e9e71fc74b2ec61',
};

export const blob1: SearchResultBlob = {
  blob_hash: '0x0108dd3e414da9f3255f7a831afa606e8dfaea93d082dfa9b15305583cbbdbbe',
  type: 'blob' as const,
  timestamp: null,
};

export const domain1: SearchResultDomain = {
  address_hash: '0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045',
  ens_info: {
    address_hash: '0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045',
    expiry_date: '2039-09-01T07:36:18.000Z',
    name: 'vitalik.eth',
    names_count: 1,
  },
  is_smart_contract_verified: false,
  is_smart_contract_address: false,
  name: null,
  type: 'ens_domain',
  url: '/address/0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045',
};

export const metatag1: SearchResultMetadataTag = {
  ...address1,
  type: 'metadata_tag',
  metadata: {
    name: 'utko',
    slug: 'utko',
    meta: {},
    tagType: 'name',
    ordinal: 1,
  },
};

export const metatag2: SearchResultMetadataTag = {
  ...address2,
  type: 'metadata_tag',
  metadata: {
    name: 'utko',
    slug: 'utko',
    meta: {
      cexDeposit: 'true',
    },
    tagType: 'name',
    ordinal: 1,
  },
};

export const metatag3: SearchResultMetadataTag = {
  ...contract2,
  type: 'metadata_tag',
  metadata: {
    name: 'super utko',
    slug: 'super-utko',
    meta: {},
    tagType: 'protocol',
    ordinal: 1,
  },
};

export const tacOperation1: SearchResultTacOperation = {
  type: 'tac_operation',
  tac_operation: tacOperationMock.tacOperation,
};

export const baseResponse: SearchResult = {
  items: [
    token1,
    token2,
    block1,
    address1,
    contract1,
    tx1,
    blob1,
    domain1,
    metatag1,
    tacOperation1,
  ],
  next_page_params: null,
};
