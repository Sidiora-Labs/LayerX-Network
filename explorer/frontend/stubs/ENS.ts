import * as bens from '@blockscout/bens-types';

import { ADDRESS_PARAMS, ADDRESS_HASH } from './addressParams';
import { TX_HASH } from './tx';

export const ENS_DOMAIN: bens.DetailedDomain = {
  id: '0x126d74db13895f8d3a1d362410212731d1e1d9be8add83e388385f93d84c8c84',
  name: 'kitty.cat.eth',
  tokens: [
    {
      id: '973523146267017920308',
      contract_hash: ADDRESS_HASH,
      type: bens.TokenType.NATIVE_DOMAIN_TOKEN,
    },
  ],
  owner: ADDRESS_PARAMS,
  resolved_address: ADDRESS_PARAMS,
  registrant: ADDRESS_PARAMS,
  registration_date: '2023-12-20T01:29:12.000Z',
  expiry_date: '2099-01-02T01:29:12.000Z',
  other_addresses: {
    ETH: ADDRESS_HASH,
  },
  protocol: undefined,
  resolved_with_wildcard: false,
  stored_offchain: false,
};

export const ENS_DOMAIN_EVENT: bens.DomainEvent = {
  transaction_hash: TX_HASH,
  timestamp: '2022-06-06T08:43:15.000000Z',
  from_address: ADDRESS_PARAMS,
  action: '0xf7a16963',
};
