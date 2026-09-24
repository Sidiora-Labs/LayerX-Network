import { Box } from '@chakra-ui/react';
import React from 'react';

import * as tac from '@blockscout/tac-operation-lifecycle-types';

import DetailedInfoTimestamp from 'ui/shared/DetailedInfo/DetailedInfoTimestamp';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TxEntityTon from 'ui/shared/entities/tx/TxEntityTon';
import { ItemContent, ItemBody, ItemRow } from 'ui/shared/lifecycle/LifecycleAccordion';
import StatusTag from 'ui/shared/statusTag/StatusTag';

interface Props {
  isLast: boolean;
  data: tac.OperationStage;
}

const TacOperationLifecycleAccordionItemContent = ({ isLast, data }: Props) => {
  return (
    <ItemContent isLast={ isLast }>
      <ItemBody>
        <ItemRow label="Status">
          <StatusTag type={ data.is_success ? 'ok' : 'error' } text={ data.is_success ? 'Success' : 'Failed' } my={ 1 }/>
        </ItemRow>

        { data.timestamp && (
          <ItemRow label="Timestamp">
            <DetailedInfoTimestamp timestamp={ data.timestamp } isLoading={ false } flexWrap={{ base: 'wrap', lg: 'nowrap' }} py="6px"/>
          </ItemRow>
        ) }

        { data.transactions.length > 0 && (
          <ItemRow label="Transactions">
            <Box
              display="flex"
              flexDirection="column"
              rowGap={ 3 }
              py="6px"
              width="100%"
              overflow="hidden"
            >
              {
                data.transactions.map((tx) => {
                  if (tx.type === tac.BlockchainType.TON) {
                    return <TxEntityTon key={ tx.hash } hash={ tx.hash }/>;
                  }

                  return <TxEntity key={ tx.hash } hash={ tx.hash } icon={{ name: 'brands/tac' }}/>;
                })
              }
            </Box>
          </ItemRow>
        ) }

        { data.note && (
          <ItemRow label="Note">
            <Box
              display="inline-flex"
              alignItems="center"
              py="6px"
              whiteSpace="pre-wrap"
              wordBreak="break-word"
            >
              { data.note }
            </Box>
          </ItemRow>
        ) }
      </ItemBody>
    </ItemContent>
  );
};

export default React.memo(TacOperationLifecycleAccordionItemContent);
