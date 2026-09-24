import type { ScrollL2MessagesResponse } from 'types/api/scrollL2';

export const baseResponse: ScrollL2MessagesResponse = {
  items: [
    {
      id: 930795,
      origination_transaction_block_number: 20639178,
      origination_transaction_hash: '0x70380f2c6ecd53aa6e0608e6c9d770acaa29c0508869ec296bae3e09678ea9f4',
      origination_timestamp: '2024-08-30T05:03:23.000000Z',
      completion_transaction_hash: null,
      value: '5084131319054877748',
    },
    {
      id: 930748,
      origination_transaction_block_number: 20638104,
      origination_transaction_hash: '0x7e7b4d5ff0b7a6af5e52f4aa2ad9eca3c0c5664368cbb781e04b5b13c6109b2b',
      origination_timestamp: '2024-08-30T01:26:35.000000Z',
      completion_transaction_hash: '0x426b16ea3a42228f6d754ae55c348986122cdb1e4331b6fd454975776f513ea1',
      value: '0',
    },
  ],
  next_page_params: {
    items_count: 50,
    id: 1,
  },
};
