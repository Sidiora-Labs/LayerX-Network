import type { ZkEvmL2WithdrawalsResponse } from 'types/api/zkEvmL2';

export const baseResponse: ZkEvmL2WithdrawalsResponse = {
  items: [
    {
      block_number: 11722417,
      index: 47040,
      l1_transaction_hash: null,
      l2_transaction_hash: '0x68c378e412e51553524545ef1d3f00f69496fb37827c0b3b7e0870d245970408',
      symbol: 'ETH',
      timestamp: '2022-04-18T09:20:37.000000Z',
      value: '0.025',
    },
    {
      block_number: 11722480,
      index: 47041,
      l1_transaction_hash: '0xbf76feb85b8b8f24dacb17f962dd359f82efc512928d7b11ffca92fb812ad6a5',
      l2_transaction_hash: '0xfe3c168ac1751b8399f1e819f1d83ee4cf764128bc604d454abee29114dabf49',
      symbol: 'ETH',
      timestamp: '2022-04-18T09:23:45.000000Z',
      value: '4',
    },
  ],
  next_page_params: {
    items_count: 50,
    index: 1,
  },
};
