import type { ArbitrumL2TxnBatchesItem, ArbitrumL2TxnBatchesResponse } from 'types/api/arbitrumL2';

export const finalized: ArbitrumL2TxnBatchesItem = {
  number: 12345,
  blocks_count: 12345,
  transactions_count: 10000,
  commitment_transaction: {
    block_number: 12345,
    timestamp: '2022-04-17T08:51:58.000000Z',
    hash: '0x262e7215739d6a7e33b2c20b45a838801a0f5f080f20bec8e54eb078420c4661',
    status: 'finalized',
  },
  batch_data_container: 'in_blob4844',
};

export const unfinalized: ArbitrumL2TxnBatchesItem = {
  number: 12344,
  blocks_count: 10000,
  transactions_count: 103020,
  commitment_transaction: {
    block_number: 12340,
    timestamp: '2022-04-17T08:51:58.000000Z',
    hash: '0x262e7215739d6a7e33b2c20b45a838801a0f5f080f20bec8e54eb078420c4661',
    status: 'unfinalized',
  },
  batch_data_container: null,

};

export const baseResponse: ArbitrumL2TxnBatchesResponse = {
  items: [
    finalized,
    unfinalized,
  ],
  next_page_params: {
    items_count: 50,
    number: 123,
  },
};
