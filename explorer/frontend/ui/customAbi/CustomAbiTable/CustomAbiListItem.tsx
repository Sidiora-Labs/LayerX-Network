import { Box } from '@chakra-ui/react';
import React, { useCallback } from 'react';

import type { CustomAbi } from 'types/api/account';

import { Skeleton } from 'toolkit/chakra/skeleton';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import ListItemMobile from 'ui/shared/ListItemMobile/ListItemMobile';
import TableItemActionButtons from 'ui/shared/TableItemActionButtons';

interface Props {
  item: CustomAbi;
  isLoading?: boolean;
  onEditClick: (item: CustomAbi) => void;
  onDeleteClick: (item: CustomAbi) => void;
}

const CustomAbiListItem = ({ item, isLoading, onEditClick, onDeleteClick }: Props) => {

  const onItemEditClick = useCallback(() => {
    return onEditClick(item);
  }, [ item, onEditClick ]);

  const onItemDeleteClick = useCallback(() => {
    return onDeleteClick(item);
  }, [ item, onDeleteClick ]);

  return (
    <ListItemMobile>
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
      <TableItemActionButtons onDeleteClick={ onItemDeleteClick } onEditClick={ onItemEditClick } isLoading={ isLoading }/>
    </ListItemMobile>
  );
};

export default React.memo(CustomAbiListItem);
