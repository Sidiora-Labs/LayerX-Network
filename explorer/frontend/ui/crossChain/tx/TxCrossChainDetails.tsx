import React from 'react';

import type { InterchainMessage } from '@blockscout/interchain-indexer-types';

import CrossChainBridgeLink from 'ui/shared/crossChain/CrossChainBridgeLink';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoTimestamp from 'ui/shared/DetailedInfo/DetailedInfoTimestamp';
import AddressEntityInterchain from 'ui/shared/entities/address/AddressEntityInterchain';
import RawInputData from 'ui/shared/RawInputData';
import CrossChainTxsStatusTag from 'ui/shared/statusTag/CrossChainTxsStatusTag';

import TxCrossChainDetailsLifecycle from './TxCrossChainDetailsLifecycle';
import TxCrossChainDetailsTransfers from './TxCrossChainDetailsTransfers';

interface Props {
  data: InterchainMessage | undefined;
  isLoading?: boolean;
}

const TxCrossChainDetails = ({ data, isLoading }: Props) => {

  if (!data) {
    return <DataFetchAlert/>;
  }

  return (
    <DetailedInfo.Container>
      { data.sender && (
        <>
          <DetailedInfo.ItemLabel
            hint="Address that initiated the cross-chain operation on the source chain"
            isLoading={ isLoading }
          >
            Sender
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <AddressEntityInterchain
              address={ data.sender }
              isLoading={ isLoading }
              chain={ data.source_chain }
            />
          </DetailedInfo.ItemValue>
        </>
      ) }
      { data.recipient && (
        <>
          <DetailedInfo.ItemLabel
            hint="Recipient address on the destination chain, usually the account or contract that receives tokens or the message"
            isLoading={ isLoading }
          >
            Target
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <AddressEntityInterchain
              address={ data.recipient }
              isLoading={ isLoading }
              chain={ data.destination_chain }
            />
          </DetailedInfo.ItemValue>
        </>
      ) }
      <DetailedInfo.ItemLabel
        hint="Cross-chain protocol used to route the message between chains (bridge or messaging layer)"
        isLoading={ isLoading }
      >
        Protocol
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <CrossChainBridgeLink data={ data.bridge } isLoading={ isLoading }/>
      </DetailedInfo.ItemValue>

      { data.transfers.length > 0 && <TxCrossChainDetailsTransfers data={ data.transfers } id={ data.message_id } isLoading={ isLoading }/> }

      <DetailedInfo.ItemLabel
        hint="Current state of the cross-chain operation"
        isLoading={ isLoading }
      >
        Status
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <CrossChainTxsStatusTag status={ data.status } loading={ isLoading } mode="full"/>
      </DetailedInfo.ItemValue>
      <DetailedInfo.ItemLabel
        hint="Date and time when the cross-chain operation was completed or initiated"
        isLoading={ isLoading }
      >
        Timestamp
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <DetailedInfoTimestamp timestamp={ data.receive_timestamp || data.send_timestamp } isLoading={ isLoading }/>
      </DetailedInfo.ItemValue>
      <DetailedInfo.ItemLabel
        hint="Source and destination transactions linked to this cross-chain operation"
        isLoading={ isLoading }
      >
        Lifecycle
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue py={ 1 }>
        <TxCrossChainDetailsLifecycle data={ data } isLoading={ isLoading }/>
      </DetailedInfo.ItemValue>
      { data.payload && (
        <>
          <DetailedInfo.ItemLabel
            hint="Message data sent as part of the cross-chain operation"
            isLoading={ isLoading }
          >
            Payload
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <RawInputData hex={ data.payload } isLoading={ isLoading }/>
          </DetailedInfo.ItemValue>
        </>
      ) }
    </DetailedInfo.Container>
  );
};

export default React.memo(TxCrossChainDetails);
