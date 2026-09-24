import type { ZkSyncBatchesItem, ZkSyncBatchesResponse } from 'types/api/zkSyncL2';

export const sealed: ZkSyncBatchesItem = {
  commit_transaction_hash: null,
  commit_transaction_timestamp: null,
  execute_transaction_hash: null,
  execute_transaction_timestamp: null,
  number: 8055,
  prove_transaction_hash: null,
  prove_transaction_timestamp: null,
  status: 'Sealed on L2',
  timestamp: '2022-03-19T12:53:36.000000Z',
  transactions_count: 738,
};

export const sent: ZkSyncBatchesItem = {
  commit_transaction_hash: '0x262e7215739d6a7e33b2c20b45a838801a0f5f080f20bec8e54eb078420c4661',
  commit_transaction_timestamp: '2022-03-19T13:09:07.357570Z',
  execute_transaction_hash: null,
  execute_transaction_timestamp: null,
  number: 8054,
  prove_transaction_hash: null,
  prove_transaction_timestamp: null,
  status: 'Sent to L1',
  timestamp: '2022-03-19T11:36:45.000000Z',
  transactions_count: 766,
};

export const executed: ZkSyncBatchesItem = {
  commit_transaction_hash: '0xa2628f477e1027ac1c60fa75c186b914647769ac1cb9c7e1cab50b13506a0035',
  commit_transaction_timestamp: '2022-03-19T11:52:18.963659Z',
  execute_transaction_hash: '0xb7bd6b2b17498c66d3f6e31ac3685133a81b7f728d4f6a6f42741daa257d0d68',
  execute_transaction_timestamp: '2022-03-19T13:28:16.712656Z',
  number: 8053,
  prove_transaction_hash: '0x9d44f2b775bd771f8a53205755b3897929aa672d2cd419b3b988c16d41d4f21e',
  prove_transaction_timestamp: '2022-03-19T13:28:16.603104Z',
  status: 'Executed on L1',
  timestamp: '2022-03-19T10:01:52.000000Z',
  transactions_count: 1071,
};

export const baseResponse: ZkSyncBatchesResponse = {
  items: [
    sealed,
    sent,
    executed,
  ],
  next_page_params: null,
};
