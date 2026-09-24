import type { ZkEvmL2DepositsResponse } from 'types/api/zkEvmL2';

export const baseResponse: ZkEvmL2DepositsResponse = {
  items: [
    {
      block_number: 19681943,
      index: 182177,
      l1_transaction_hash: '0x29074452f976064aca1ca5c6e7c82d890c10454280693e6eca0257ae000c8e85',
      l2_transaction_hash: null,
      symbol: 'DAI',
      timestamp: '2022-04-18T11:08:11.000000Z',
      value: '0.003',
    },
    {
      block_number: 19681894,
      index: 182176,
      l1_transaction_hash: '0x0b7d58c0a6b4695ba28d99df928591fb931c812c0aab6d0093ff5040d2f9bc5e',
      l2_transaction_hash: '0x210d9f70f411de1079e32a98473b04345a5ea6ff2340a8511ebc2df641274436',
      symbol: 'ETH',
      timestamp: '2022-04-18T10:58:23.000000Z',
      value: '0.0046651390188845',
    },
  ],
  next_page_params: {
    items_count: 50,
    index: 1,
  },
};
