import { Box } from '@chakra-ui/react';
import React, { useCallback } from 'react';

import type { CustomAbi } from 'types/api/account';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import TableItemActionButtons from 'ui/shared/TableItemActionButtons';

interface Props {
  item: CustomAbi;
  isLoading?: boolean;
  onEditClick: (item: CustomAbi) => void;
  onDeleteClick: (item: CustomAbi) => void;
}

const CustomAbiTableItem = ({ item, isLoading, onEditClick, onDeleteClick }: Props) => {

  const onItemEditClick = useCallback(() => {
    return onEditClick(item);
  }, [ item, onEditClick ]);

  const onItemDeleteClick = useCallback(() => {
    return onDeleteClick(item);
  }, [ item, onDeleteClick ]);

  return (
    <TableRow alignItems="top" key={ item.id }>
      <TableCell>
        <Box maxW="100%">
          <AddressEntity
            address={ item.contract_address }
            fontWeight="600"
            isLoading={ isLoading }
          />
          <Skeleton textStyle="sm" color="text.secondary" mt={ 0.5 } ml={ 8 } display="inline-block" loading={ isLoading }>
            <span>{ item.name }</span>
          </Skeleton>
        </Box>
      </TableCell>
      <TableCell>
        <TableItemActionButtons onDeleteClick={ onItemDeleteClick } onEditClick={ onItemEditClick } isLoading={ isLoading }/>
      </TableCell>
    </TableRow>
  );
};

export default React.memo(CustomAbiTableItem);
