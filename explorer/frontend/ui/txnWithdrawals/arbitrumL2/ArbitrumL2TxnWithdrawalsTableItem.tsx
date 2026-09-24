import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { ArbitrumL2TxnWithdrawalsItem } from 'types/api/arbitrumL2';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressEntityL1 from 'ui/shared/entities/address/AddressEntityL1';
import ArbitrumL2MessageStatus from 'ui/shared/statusTag/ArbitrumL2MessageStatus';

import ArbitrumL2TxnWithdrawalsClaimButton from './ArbitrumL2TxnWithdrawalsClaimButton';
import ArbitrumL2TxnWithdrawalsValue from './ArbitrumL2TxnWithdrawalsValue';

interface Props {
  txHash: string | undefined;
  data: ArbitrumL2TxnWithdrawalsItem;
  isLoading?: boolean;
}

const ArbitrumL2TxnWithdrawalsTableItem = ({ data, isLoading, txHash }: Props) => {
  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading }>{ data.id }</Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        <AddressEntityL1 address={{ hash: data.token?.destination_address_hash || data.destination_address_hash }} isLoading={ isLoading }/>
      </TableCell>
      <TableCell verticalAlign="middle" isNumeric>
        <ArbitrumL2TxnWithdrawalsValue data={ data } loading={ isLoading }/>
      </TableCell>
      <TableCell verticalAlign="middle">
        <Flex alignItems="center" justifyContent="space-between" columnGap={ 8 }>
          <ArbitrumL2MessageStatus status={ data.status } isLoading={ isLoading }/>
          { (data.status === 'confirmed' || data.status === 'relayed') && (
            <ArbitrumL2TxnWithdrawalsClaimButton
              messageId={ data.id }
              txHash={ txHash }
              completionTxHash={ data.completion_transaction_hash || undefined }
              isLoading={ isLoading }
            />
          ) }
        </Flex>
      </TableCell>
    </TableRow>
  );
};

export default React.memo(ArbitrumL2TxnWithdrawalsTableItem);
