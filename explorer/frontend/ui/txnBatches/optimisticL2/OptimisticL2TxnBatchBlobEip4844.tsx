import { GridItem, VStack } from '@chakra-ui/react';
import React from 'react';

import type { OptimisticL2BlobTypeEip4844 } from 'types/api/optimisticL2';

import { layerLabels } from 'lib/rollups/utils';
import DetailedInfoTimestamp from 'ui/shared/DetailedInfo/DetailedInfoTimestamp';
import BlobEntityL1 from 'ui/shared/entities/blob/BlobEntityL1';
import TxEntityL1 from 'ui/shared/entities/tx/TxEntityL1';

import OptimisticL2TxnBatchBlobWrapper from './OptimisticL2TxnBatchBlobWrapper';

interface Props {
  blobs: Array<OptimisticL2BlobTypeEip4844>;
  isLoading: boolean;
}

const OptimisticL2TxnBatchBlobEip4844 = ({ blobs, isLoading }: Props) => {
  return (
    <VStack rowGap={ 2 } w="100%">
      { blobs.map((blob) => {
        return (
          <OptimisticL2TxnBatchBlobWrapper key={ blob.hash } isLoading={ isLoading }>
            <GridItem fontWeight={ 600 }>Versioned hash</GridItem>
            <GridItem overflow="hidden">
              <BlobEntityL1 hash={ blob.hash }/>
            </GridItem>
            <GridItem fontWeight={ 600 }>Timestamp</GridItem>
            <GridItem overflow="hidden">
              <DetailedInfoTimestamp timestamp={ blob.l1_timestamp } isLoading={ isLoading } flexWrap={{ base: 'wrap', lg: 'nowrap' }}/>
            </GridItem>
            <GridItem fontWeight={ 600 }>{ layerLabels.parent } txn hash</GridItem>
            <GridItem overflow="hidden">
              <TxEntityL1 hash={ blob.l1_transaction_hash } noIcon/>
            </GridItem>
          </OptimisticL2TxnBatchBlobWrapper>
        );
      }) }
    </VStack>

  );
};

export default React.memo(OptimisticL2TxnBatchBlobEip4844);
