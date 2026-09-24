import React from 'react';

import type { ZkEvmL2TxnBatchesItem } from 'types/api/zkEvmL2';

import { layerLabels } from 'lib/rollups/utils';

import type { StatusTagType } from './StatusTag';
import StatusTag from './StatusTag';

export interface Props {
  status: ZkEvmL2TxnBatchesItem['status'];
  isLoading?: boolean;
}

const ZkEvmL2TxnBatchStatus = ({ status, isLoading }: Props) => {
  let type: StatusTagType;
  let text: string;

  switch (status) {
    case 'L1 Sequence Confirmed': {
      type = 'ok';
      text = `${ layerLabels.parent } sequence confirmed`;
      break;
    }
    case 'Finalized': {
      type = 'ok';
      text = status;
      break;
    }
    default: {
      type = 'pending';
      text = status;
    }
  }

  return <StatusTag type={ type } text={ text } loading={ isLoading }/>;
};

export default ZkEvmL2TxnBatchStatus;
