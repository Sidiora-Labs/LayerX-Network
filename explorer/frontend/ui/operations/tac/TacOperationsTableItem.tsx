import React from 'react';

import type * as tac from '@blockscout/tac-operation-lifecycle-types';

import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressEntityTacTon from 'ui/shared/entities/address/AddressEntityTacTon';
import OperationEntity from 'ui/shared/entities/operation/OperationEntity';
import TacOperationStatus from 'ui/shared/statusTag/TacOperationStatus';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
interface Props {
  item: tac.OperationBriefDetails;
  isLoading?: boolean;
}

const TacOperationsTableItem = ({ item, isLoading }: Props) => {
  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <TacOperationStatus status={ item.type } isLoading={ isLoading }/>
      </TableCell>
      <TableCell verticalAlign="middle">
        <OperationEntity
          id={ item.operation_id }
          type={ item.type }
          isLoading={ isLoading }
          truncation="constant_long"
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        <TimeWithTooltip
          timestamp={ item.timestamp }
          isLoading={ isLoading }
          color="text.secondary"
        />
      </TableCell>
      <TableCell verticalAlign="middle" pr={ 12 }>
        { item.sender ? (
          <AddressEntityTacTon
            address={{ hash: item.sender.address }}
            chainType={ item.sender.blockchain }
            truncation="constant"
            isLoading={ isLoading }
            w="fit-content"
          />
        ) : '-' }
      </TableCell>
    </TableRow>
  );
};

export default TacOperationsTableItem;
