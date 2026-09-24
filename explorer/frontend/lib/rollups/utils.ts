import type { ZKEVM_L2_TX_STATUSES } from 'types/api/transaction';
import type { ZKEVM_L2_TX_BATCH_STATUSES } from 'types/api/zkEvmL2';
import type { ZKSYNC_L2_TX_BATCH_STATUSES } from 'types/api/zkSyncL2';

import config from 'configs/app';

const feature = config.features.rollup;

export const layerLabels = feature.isEnabled ? {
  current: `L${ feature.layerNumber }`,
  parent: `L${ feature.layerNumber - 1 }`,
} : {
  current: 'L2',
  parent: 'L1',
};

export const formatZkEvmTxStatus = (status: typeof ZKEVM_L2_TX_STATUSES[number]) => {
  switch (status) {
    case 'L1 Confirmed':
      return `${ layerLabels.parent } Confirmed`;
    default:
      return status;
  }
};

export const formatZkEvmL2TxnBatchStatus = (status: typeof ZKEVM_L2_TX_BATCH_STATUSES[number]) => {
  switch (status) {
    case 'L1 Sequence Confirmed':
      return `${ layerLabels.parent } Sequence Confirmed`;
    default:
      return status;
  }
};

export const formatZkSyncL2TxnBatchStatus = (status: typeof ZKSYNC_L2_TX_BATCH_STATUSES[number]) => {
  return status.replace('L2', layerLabels.current).replace('L1', layerLabels.parent);
};
