import React, { useCallback } from 'react';

import type { ApiKey } from 'types/api/account';

import { TableCell, TableRow } from 'toolkit/chakra/table';
import ApiKeySnippet from 'ui/shared/ApiKeySnippet';
import TableItemActionButtons from 'ui/shared/TableItemActionButtons';

interface Props {
  item: ApiKey;
  isLoading?: boolean;
  onEditClick: (item: ApiKey) => void;
  onDeleteClick: (item: ApiKey) => void;
}

const ApiKeyTableItem = ({ item, isLoading, onEditClick, onDeleteClick }: Props) => {

  const onItemEditClick = useCallback(() => {
    return onEditClick(item);
  }, [ item, onEditClick ]);

  const onItemDeleteClick = useCallback(() => {
    return onDeleteClick(item);
  }, [ item, onDeleteClick ]);

  return (
    <TableRow alignItems="top" key={ item.api_key }>
      <TableCell>
        <ApiKeySnippet apiKey={ item.api_key } name={ item.name } isLoading={ isLoading }/>
      </TableCell>
      <TableCell>
        <TableItemActionButtons onDeleteClick={ onItemDeleteClick } onEditClick={ onItemEditClick } isLoading={ isLoading }/>
      </TableCell>
    </TableRow>
  );
};

export default ApiKeyTableItem;
