import * as tac from '@blockscout/tac-operation-lifecycle-types';

import { ADDRESS_HASH } from './addressParams';

export const TAC_OPERATION: tac.OperationBriefDetails = {
  operation_id: '0x4d3d36b7fcab0a2f93f24bf313ebfe9cc0b2c7157d2aef7e7f7d5835528428c6',
  type: tac.OperationType.TAC_TON,
  timestamp: '2025-05-05T12:32:22.000Z',
  sender: {
    address: '0x4d3d36b7fcab0a2f93f24bf313ebfe9cc0b2c7157d2aef7e7f7d5835528428c6',
    blockchain: tac.BlockchainType.TAC,
  },
};

export const TAC_OPERATION_DETAILS: tac.OperationDetails = {
  operation_id: '0x6e7cdeea3f39e7664597a44ddb33ce47ba061cbee2992e2c7b0e3f9294ff8b30',
  type: tac.OperationType.TAC_TON,
  timestamp: '2025-05-05T12:32:22.000Z',
  sender: {
    address: ADDRESS_HASH,
    blockchain: tac.BlockchainType.TAC,
  },
  status_history: [
    {
      type: tac.OperationStage_StageType.COLLECTED_IN_TAC,
      is_exist: true,
      is_success: true,
      timestamp: '2025-05-05T12:32:22.000Z',
      transactions: [
        {
          hash: '0x064e57a9f43d032ac0c1cb0d7883b0d783a9fa5d207a39563a6ed06c5dc17622',
          type: tac.BlockchainType.TON,
        },
      ],
      note: undefined,
    },
  ],
};
