import type * as multichain from '@blockscout/multichain-aggregator-types';

import { chainA, chainB, chainC } from './chains';

export const base: multichain.GetAddressPortfolioResponse = {
  portfolio: {
    total_value: (128147.328 + 73107.04593748 + 17675.3 + 217.17267).toFixed(4),
    chain_values: {
      [chainA.id]: (128147.328 + 73107.04593748).toFixed(4),
      [chainB.id]: 17675.3.toFixed(4),
      [chainC.id]: 217.17267.toFixed(4),
    },
  },
};

export const zero: multichain.GetAddressPortfolioResponse = {
  portfolio: {
    total_value: '0.0000',
    chain_values: {},
  },
};
