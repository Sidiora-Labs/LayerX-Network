import * as tac from '@blockscout/tac-operation-lifecycle-types';

export const tacOperation: tac.OperationDetails = {
  operation_id: '0x35f5d9c2bf07477ede48935c7130945faf17a3e5f69a7d20ce3725676513095c',
  type: tac.OperationType.TON_TAC_TON,
  timestamp: '2025-05-08T07:20:05.000Z',
  sender: {
    address: 'EQBnVg4x6uTCa8jlrh8YXyWpnJJ3oxxrdBQ2+Zw8yaoxnXTt',
    blockchain: tac.BlockchainType.TON,
  },
  status_history: [
    {
      type: tac.OperationStage_StageType.COLLECTED_IN_TON,
      is_exist: true,
      is_success: true,
      timestamp: '2025-05-08T07:20:05.000Z',
      transactions: [
        {
          hash: '0x77e3c6bef84681157dda17dec60f680a1ff6caaedec2e94c23f4ec44aa62aba8',
          type: tac.BlockchainType.TON,
        },
      ],
      note: undefined,
    },
    {
      type: tac.OperationStage_StageType.INCLUDED_IN_TON_CONSENSUS,
      is_exist: true,
      is_success: true,
      timestamp: '2025-05-08T07:25:35.000Z',
      transactions: [
        {
          hash: '0xafc8a8e04739b4996e9b5ef6c91673fb421d00ed42be4404d6fca6a915899235',
          type: tac.BlockchainType.TAC,
        },
        {
          hash: '0xafc8a8e04739b4996e9b5ef6c91673fb421d00ed42be4404d6fca6a915899236',
          type: tac.BlockchainType.TON,
        },
      ],
      note: undefined,
    },
    {
      type: tac.OperationStage_StageType.EXECUTED_IN_TON,
      is_exist: true,
      is_success: false,
      timestamp: '2025-05-08T07:26:14.000Z',
      transactions: [
        {
          hash: '0xa9c6087ee95ede3cb0bcba7119a7f7b0ee3fc91d04faa1bb1ecd94ed83ef8161',
          type: tac.BlockchainType.TAC,
        },
      ],
      note: 'ProxyCallError: UniswapV2Router: Insufficient output amount',
    },
  ],
};
