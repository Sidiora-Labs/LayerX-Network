import type { ArbitrumL2MessagesResponse, ArbitrumLatestDepositsResponse } from 'types/api/arbitrumL2';

export const baseResponse: ArbitrumL2MessagesResponse = {
  items: [
    {
      completion_transaction_hash: '0x0b7d58c0a6b4695ba28d99df928591fb931c812c0aab6d0093ff5040d2f9bc5e',
      id: 181920,
      origination_address_hash: '0x2B51Ae4412F79c3c1cB12AA40Ea4ECEb4e80511a',
      origination_transaction_block_number: 123456,
      origination_transaction_hash: '0x210d9f70f411de1079e32a98473b04345a5ea6ff2340a8511ebc2df641274436',
      origination_timestamp: '2023-06-01T14:46:48.000000Z',
      status: 'initiated',
    },
    {
      completion_transaction_hash: '0x0b7d58c0a6b4695ba28d99df928591fb931c812c0aab6d0093ff5040d2f9bc5e',
      id: 181921,
      origination_address_hash: '0x2B51Ae4412F79c3c1cB12AA40Ea4ECEb4e80511a',
      origination_transaction_block_number: 123400,
      origination_transaction_hash: '0x210d9f70f411de1079e32a98473b04345a5ea6ff2340a8511ebc2df641274436',
      origination_timestamp: '2023-06-01T14:46:48.000000Z',
      status: 'relayed',
    },
  ],
  next_page_params: {
    items_count: 50,
    id: 123,
    direction: 'to-rollup',
  },
};

export const latestDepositsResponse: ArbitrumLatestDepositsResponse = {
  items: [
    {
      completion_transaction_hash: '0x3ccdf87449d3de6a9dcd3eddb7bc9ecdf1770d4631f03cdf12a098911618d138',
      origination_transaction_block_number: 123400,
      origination_transaction_hash: '0x210d9f70f411de1079e32a98473b04345a5ea6ff2340a8511ebc2df641274436',
      origination_timestamp: '2023-06-01T14:46:48.000000Z',
    },
    {
      completion_transaction_hash: '0xd16d918b2f95a5cdf66824f6291b6d5eb80b6f4acab3f9fb82ee0ec4109646a0',
      origination_timestamp: null,
      origination_transaction_block_number: null,
      origination_transaction_hash: null,
    },
  ],
};
