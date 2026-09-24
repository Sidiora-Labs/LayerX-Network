import React from 'react';

import type { ArbitrumL2TxnWithdrawalsItem } from 'types/api/arbitrumL2';

import config from 'configs/app';
import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressEntityL1 from 'ui/shared/entities/address/AddressEntityL1';
import ListItemMobileGrid from 'ui/shared/ListItemMobile/ListItemMobileGrid';
import ArbitrumL2MessageStatus from 'ui/shared/statusTag/ArbitrumL2MessageStatus';

import ArbitrumL2TxnWithdrawalsClaimButton from './ArbitrumL2TxnWithdrawalsClaimButton';
import ArbitrumL2TxnWithdrawalsValue from './ArbitrumL2TxnWithdrawalsValue';

const rollupFeature = config.features.rollup;

interface Props {
  data: ArbitrumL2TxnWithdrawalsItem;
  isLoading: boolean;
  txHash: string | undefined;
}

const ArbitrumL2TxnWithdrawalsListItem = ({ data, isLoading, txHash }: Props) => {
  if (!rollupFeature.isEnabled || rollupFeature.type !== 'arbitrum') {
    return null;
  }

  return (
    <ListItemMobileGrid.Container gridTemplateColumns="110px auto">

      <ListItemMobileGrid.Label isLoading={ isLoading }>Message #</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <Skeleton loading={ isLoading }>{ data.id }</Skeleton>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Receiver</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <AddressEntityL1 address={{ hash: data.token?.destination_address_hash || data.destination_address_hash }} isLoading={ isLoading }/>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Value</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <Skeleton loading={ isLoading }>
          <ArbitrumL2TxnWithdrawalsValue data={ data }/>
        </Skeleton>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Status</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value
        display="flex"
        alignItems="center"
        justifyContent="space-between"
        flexWrap="wrap"
        rowGap={ 2 }
        columnGap={ 3 }
        py={ 0 }
      >
        <ArbitrumL2MessageStatus status={ data.status } isLoading={ isLoading }/>
        { (data.status === 'confirmed' || data.status === 'relayed') && (
          <ArbitrumL2TxnWithdrawalsClaimButton
            messageId={ data.id }
            txHash={ txHash }
            completionTxHash={ data.completion_transaction_hash || undefined }
            isLoading={ isLoading }
          />
        ) }
      </ListItemMobileGrid.Value>

    </ListItemMobileGrid.Container>
  );
};

export default ArbitrumL2TxnWithdrawalsListItem;
