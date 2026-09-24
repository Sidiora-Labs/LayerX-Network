import type { ScrollL2BatchesResponse } from 'types/api/scrollL2';

export const batchData = {
  number: 66928,
  commitment_transaction: {
    block_number: 19114878,
    hash: '0x57552c0dbcf56383ee2efdf8fd6be143b355135fc300361924582c308877b8b7',
    timestamp: '2024-01-29T21:31:35.000000Z',
  },
  confirmation_transaction: {
    block_number: null,
    hash: null,
    timestamp: null,
  },
  data_availability: {
    batch_data_container: 'in_blob4844' as const,
  },
  start_block_number: 456000,
  end_block_number: 789000,
  transactions_count: 654,
};

export const baseResponse: ScrollL2BatchesResponse = {
  items: [
    batchData,
    {
      number: 66879,
      commitment_transaction: {
        block_number: 19114386,
        hash: '0x0d33245814b9e61c8f0ed6fd3fb7464f34be33d2c3aee69629d65e8995d77edc',
        timestamp: '2024-01-29T19:52:35.000000Z',
      },
      confirmation_transaction: {
        block_number: 19114558,
        hash: '0x6f9a19d503947ec91d6e9d5c2129913a7def86fd0f87061c06e5994cf857bee0',
        timestamp: '2024-01-29T20:27:11.000000Z',
      },
      data_availability: {
        batch_data_container: 'in_calldata',
      },
      start_block_number: 456000,
      end_block_number: 789000,
      transactions_count: 962,
    },
  ],
  next_page_params: {
    items_count: 50,
    number: 1,
  },
};
