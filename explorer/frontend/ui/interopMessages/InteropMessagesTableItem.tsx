import React from 'react';

import type { InteropMessage } from 'types/api/interop';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressFromToIcon from 'ui/shared/address/AddressFromToIcon';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import AddressEntityInterop from 'ui/shared/entities/address/AddressEntityInterop';
import InteropMessageStatus from 'ui/shared/statusTag/InteropMessageStatus';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

import InteropMessageAdditionalInfo from './InteropMessageAdditionalInfo';
import InteropMessageDestinationTx from './InteropMessageDestinationTx';
import InteropMessageSourceTx from './InteropMessageSourceTx';

interface Props {
  item: InteropMessage;
  isLoading?: boolean;
}

const InteropMessagesTableItem = ({ item, isLoading }: Props) => {
  return (
    <TableRow>
      <TableCell>
        <InteropMessageAdditionalInfo payload={ item.payload } isLoading={ isLoading }/>
      </TableCell>
      <TableCell>
        <Skeleton loading={ isLoading } fontWeight="700">
          { item.nonce }
        </Skeleton>
      </TableCell>
      <TableCell>
        <TimeWithTooltip timestamp={ item.timestamp } isLoading={ isLoading } color="text.secondary"/>
      </TableCell>
      <TableCell>
        <InteropMessageStatus status={ item.status } isLoading={ isLoading }/>
      </TableCell>
      <TableCell>
        <InteropMessageSourceTx { ...item } isLoading={ isLoading }/>
      </TableCell>
      <TableCell>
        <InteropMessageDestinationTx { ...item } isLoading={ isLoading }/>
      </TableCell>
      <TableCell>
        { item.init_chain !== undefined ? (
          <AddressEntityInterop
            address={{ hash: item.target_address_hash }}
            isLoading={ isLoading } truncation="constant"
            chain={ item.init_chain }
            w="min-content"
          />
        ) :
          <AddressEntity address={{ hash: item.target_address_hash }} isLoading={ isLoading } truncation="constant"/>
        }
      </TableCell>
      <TableCell>
        <AddressFromToIcon
          isLoading={ isLoading }
          type={ item.init_chain !== undefined ? 'in' : 'out' }
        />
      </TableCell>
      <TableCell>
        { item.relay_chain !== undefined ? (
          <AddressEntityInterop
            address={{ hash: item.target_address_hash }}
            isLoading={ isLoading }
            truncation="constant"
            chain={ item.relay_chain }
            w="min-content"
          />
        ) :
          <AddressEntity address={{ hash: item.target_address_hash }} isLoading={ isLoading } truncation="constant"/>
        }
      </TableCell>
    </TableRow>
  );
};

export default React.memo(InteropMessagesTableItem);
