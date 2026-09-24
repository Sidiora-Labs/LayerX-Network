import React from 'react';

import type { OptimisticL2TxnBatchesItem } from 'types/api/optimisticL2';
import type { ExcludeUndefined } from 'types/utils';

import type { BadgeProps } from 'toolkit/chakra/badge';
import { Badge } from 'toolkit/chakra/badge';

export interface Props extends BadgeProps {
  container: ExcludeUndefined<OptimisticL2TxnBatchesItem['batch_data_container']>;
  isLoading?: boolean;
}

const OptimisticL2TxnBatchDA = ({ container, isLoading, ...rest }: Props) => {

  const text = (() => {
    switch (container) {
      case 'in_blob4844':
        return 'EIP-4844 blob';
      case 'in_calldata':
        return 'Calldata';
      case 'in_celestia':
        return 'Celestia blob';
      case 'in_eigenda':
        return 'EigenDA';
    }
  })();

  if (!text) {
    return null;
  }

  return (
    <Badge colorPalette="yellow" loading={ isLoading } { ...rest }>
      { text }
    </Badge>
  );
};

export default React.memo(OptimisticL2TxnBatchDA);
