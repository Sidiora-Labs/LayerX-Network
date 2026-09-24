import { Box } from '@chakra-ui/react';
import React from 'react';

import type { TxStateChange } from 'types/api/txStateChanges';

import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';

import { getStateElements } from './utils';

interface Props {
  data: TxStateChange;
  isLoading?: boolean;
}

const TxStateTableItem = ({ data, isLoading }: Props) => {
  const { before, after, change, tag, tokenId } = getStateElements(data, isLoading);

  return (
    <TableRow>
      <TableCell>
        <Box py="3px">
          { tag }
        </Box>
      </TableCell>
      <TableCell>
        <AddressEntity
          address={ data.address }
          isLoading={ isLoading }
          truncation="constant"
          my="7px"
        />
      </TableCell>
      <TableCell isNumeric><Box py="7px">{ before }</Box></TableCell>
      <TableCell isNumeric><Box py="7px">{ after }</Box></TableCell>
      <TableCell isNumeric><Box py="7px">{ change }</Box></TableCell>
      <TableCell>{ tokenId }</TableCell>
    </TableRow>
  );
};

export default React.memo(TxStateTableItem);
