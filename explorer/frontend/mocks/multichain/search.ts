import type * as multichain from '@blockscout/multichain-aggregator-types';

import { chainA } from './chains';

export const searchAddressesA: multichain.GetAddressResponse = {
  hash: '0x0000000002C5fE54822a1eD058AE2F937Fd42769',
  chain_infos: {
    [chainA.id]: {
      coin_balance: '0',
      is_contract: false,
      is_verified: false,
      contract_name: undefined,
    },
  },
  has_tokens: false,
  has_interop_message_transfers: false,
  coin_balance: '0',
  exchange_rate: '123.456',
  domains: [],
};

export const searchAddressesB: multichain.GetAddressResponse = {
  hash: '0x00883b68A6EcF2ea3D47BD735E5125a0B7625B53',
  chain_infos: {
    [chainA.id]: {
      coin_balance: '0',
      is_contract: true,
      is_verified: true,
      contract_name: 'USDT',
    },
  },
  has_tokens: false,
  has_interop_message_transfers: false,
  coin_balance: '0',
  exchange_rate: '123.456',
  domains: [],
};

export const searchTokenA: multichain.AggregatedTokenInfo = {
  address_hash: '0x94b008aA00579c1307B0EF2c499aD98a8ce58e58',
  circulating_market_cap: '162513452583.22',
  decimals: '6',
  holders_count: '1148927',
  icon_url: undefined,
  name: 'Tether USD',
  symbol: 'USDT',
  total_supply: '1000000',
  type: 'ERC-20' as multichain.TokenType,
  exchange_rate: '1.00',
  chain_infos: {
    [chainA.id]: {
      holders_count: '1148927',
      total_supply: '1000000',
      is_verified: false,
      contract_name: undefined,
    },
  },
};
