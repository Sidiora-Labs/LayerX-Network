import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { InternalTransaction } from 'types/api/internalTransaction';
import type { ClusterChainConfig } from 'types/multichain';

import { Badge } from 'toolkit/chakra/badge';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressFromTo from 'ui/shared/address/AddressFromTo';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import TxStatus from 'ui/shared/statusTag/TxStatus';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';
import { TX_INTERNALS_ITEMS } from 'ui/tx/internals/utils';

type Props = InternalTransaction & { currentAddress?: string; isLoading?: boolean; showBlockInfo?: boolean; chainData?: ClusterChainConfig };

const InternalTxsTableItem = ({
  type,
  from,
  to,
  value,
  success,
  error,
  created_contract: createdContract,
  transaction_hash: txnHash,
  block_number: blockNumber,
  timestamp,
  currentAddress,
  isLoading,
  showBlockInfo = true,
  chainData,
}: Props) => {
  const typeTitle = TX_INTERNALS_ITEMS.find(({ id }) => id === type)?.title;
  const toData = to ? to : createdContract;

  return (
    <TableRow alignItems="top">
      { chainData && (
        <TableCell>
          <ChainIcon data={ chainData } isLoading={ isLoading } my="2px"/>
        </TableCell>
      ) }
      <TableCell>
        <Flex rowGap={ 3 } flexDir="column" my="2px">
          <TxEntity
            hash={ txnHash }
            isLoading={ isLoading }
            fontWeight={ 700 }
            noIcon
            truncation="constant_long"
          />
          <TimeWithTooltip
            timestamp={ timestamp }
            enableIncrement
            isLoading={ isLoading }
            color="text.secondary"
            fontWeight="400"
            fontSize="sm"
            w="fit-content"
          />
        </Flex>
      </TableCell>
      <TableCell>
        <Flex rowGap={ 2 } flexDir="column">
          { typeTitle && (
            <Badge colorPalette="cyan" loading={ isLoading }>{ typeTitle }</Badge>
          ) }
          { !success && <TxStatus status="error" errorText={ error } isLoading={ isLoading }/> }
        </Flex>
      </TableCell>
      { showBlockInfo && (
        <TableCell>
          <BlockEntity
            isLoading={ isLoading }
            number={ blockNumber }
            noIcon
            textStyle="sm"
            fontWeight={ 500 }
            my="2px"
          />
        </TableCell>
      ) }
      <TableCell>
        <AddressFromTo
          from={ from }
          to={ toData }
          current={ currentAddress }
          isLoading={ isLoading }
          my="2px"
        />
      </TableCell>
      <TableCell isNumeric>
        <NativeCoinValue
          amount={ value }
          noSymbol
          accuracy={ 0 }
          loading={ isLoading }
          minW={ 6 }
          my="2px"
        />
      </TableCell>
    </TableRow>
  );
};

export default React.memo(InternalTxsTableItem);
