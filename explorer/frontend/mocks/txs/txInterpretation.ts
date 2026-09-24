import type { TxInterpretationResponse } from 'types/api/txInterpretation';

import { hash } from 'mocks/address/address';

export const txInterpretation: TxInterpretationResponse = {
  data: {
    summaries: [ {
      summary_template: `{action_type} {amount} {token} to {to_address} on {timestamp}`,
      summary_template_variables: {
        action_type: { type: 'string', value: 'Transfer' },
        amount: { type: 'currency', value: '100' },
        token: {
          type: 'token',
          value: {
            name: 'Duck',
            type: 'ERC-20',
            symbol: 'DUCK',
            address_hash: '0x486a3c5f34cDc4EF133f248f1C81168D78da52e8',
            holders_count: '1152',
            decimals: '18',
            icon_url: null,
            total_supply: '210000000000000000000000000',
            exchange_rate: null,
            circulating_market_cap: null,
            reputation: 'ok',
          },
        },
        to_address: {
          type: 'address',
          value: {
            hash: hash,
            implementations: null,
            is_contract: false,
            is_verified: false,
            name: null,
            private_tags: [],
            public_tags: [],
            watchlist_names: [],
            ens_domain_name: null,
          },
        },
        timestamp: {
          type: 'timestamp',
          value: '1687005431',
        },
      },
    } ],
  },
};
