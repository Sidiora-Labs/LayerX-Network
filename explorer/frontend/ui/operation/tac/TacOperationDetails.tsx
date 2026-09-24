import React from 'react';

import type * as tac from '@blockscout/tac-operation-lifecycle-types';

import { sortStatusHistory } from 'lib/operations/tac';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoTimestamp from 'ui/shared/DetailedInfo/DetailedInfoTimestamp';
import AddressEntityTacTon from 'ui/shared/entities/address/AddressEntityTacTon';
import TacOperationStatus from 'ui/shared/statusTag/TacOperationStatus';

import TacOperationLifecycleAccordion from './TacOperationLifecycleAccordion';

interface Props {
  isLoading?: boolean;
  data: tac.OperationDetails;
}

const TacOperationDetails = ({ isLoading, data }: Props) => {

  const statusHistory = data.status_history.filter((item) => item.is_exist).sort(sortStatusHistory);

  return (
    <DetailedInfo.Container
      templateColumns={{ base: 'minmax(0, 1fr)', lg: '210px minmax(728px, auto)' }}
    >
      { data?.sender && (
        <>
          <DetailedInfo.ItemLabel
            hint="The address on the source chain that starts a cross‑chain operation"
            isLoading={ isLoading }
          >
            Sender
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <AddressEntityTacTon
              address={{ hash: data.sender.address }}
              chainType={ data.sender.blockchain }
              isLoading={ isLoading }
            />
          </DetailedInfo.ItemValue>
        </>
      ) }

      <DetailedInfo.ItemLabel
        hint="The status of the operation"
        isLoading={ isLoading }
      >
        Status
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <TacOperationStatus status={ data.type } isLoading={ isLoading }/>
      </DetailedInfo.ItemValue>

      { data.timestamp && (
        <>
          <DetailedInfo.ItemLabel
            hint="Block time on the source chain when a cross‑chain operation is formed and sent"
            isLoading={ isLoading }
          >
            Timestamp
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <DetailedInfoTimestamp timestamp={ data.timestamp } isLoading={ isLoading }/>
          </DetailedInfo.ItemValue>
        </>
      ) }

      { statusHistory.length > 0 && (
        <>
          <DetailedInfo.ItemLabel
            hint="Stages of a cross‑chain operation"
            isLoading={ isLoading }
          >
            Lifecycle
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue mt={ 1 }>
            <TacOperationLifecycleAccordion data={ statusHistory } isLoading={ isLoading } type={ data.type }/>
          </DetailedInfo.ItemValue>
        </>
      ) }
    </DetailedInfo.Container>
  );
};

export default React.memo(TacOperationDetails);
