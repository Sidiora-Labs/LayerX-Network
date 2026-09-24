import React from 'react';

import type { ZkSyncBatchStatus } from 'types/api/zkSyncL2';

import { layerLabels } from 'lib/rollups/utils';

import type { StatusTagType } from './StatusTag';
import StatusTag from './StatusTag';

export interface Props {
  status: ZkSyncBatchStatus;
  isLoading?: boolean;
}

const ZkSyncL2TxnBatchStatus = ({ status, isLoading }: Props) => {
  let type: StatusTagType;

  switch (status) {
    case 'Executed on L1':
      type = 'ok';
      break;
    default:
      type = 'pending';
      break;
  }

  const text = status.replace('L1', layerLabels.parent).replace('L2', layerLabels.current);

  return <StatusTag type={ type } text={ text } loading={ isLoading }/>;
};

export default ZkSyncL2TxnBatchStatus;
