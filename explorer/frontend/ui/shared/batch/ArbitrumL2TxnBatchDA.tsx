import React from 'react';

import type { ArbitrumL2TxnBatchesItem } from 'types/api/arbitrumL2';

import { Badge } from 'toolkit/chakra/badge';

export interface Props {
  dataContainer: ArbitrumL2TxnBatchesItem['batch_data_container'];
  isLoading?: boolean;
}

const ArbitrumL2TxnBatchDA = ({ dataContainer, isLoading }: Props) => {
  let text: string;

  if (dataContainer === null) {
    return null;
  }

  switch (dataContainer) {
    case 'in_blob4844':
      text = 'Blob';
      break;
    case 'in_anytrust':
      text = 'AnyTrust';
      break;
    case 'in_calldata':
      text = 'Calldata';
      break;
    case 'in_celestia':
      text = 'Celestia';
      break;
    default:
      text = '';
  }

  if (!text) {
    return null;
  }

  return (
    <Badge loading={ isLoading } colorPalette={ dataContainer === 'in_blob4844' ? 'yellow' : 'gray' }>
      { text }
    </Badge>
  );
};

export default ArbitrumL2TxnBatchDA;
