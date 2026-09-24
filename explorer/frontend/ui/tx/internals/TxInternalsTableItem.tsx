import { Box, Flex } from '@chakra-ui/react';
import React from 'react';

import type { InternalTransaction } from 'types/api/internalTransaction';

import { Badge } from 'toolkit/chakra/badge';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressFromTo from 'ui/shared/address/AddressFromTo';
import TxStatus from 'ui/shared/statusTag/TxStatus';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';
import { TX_INTERNALS_ITEMS } from 'ui/tx/internals/utils';

type Props = InternalTransaction & {
  isLoading?: boolean;
};

const TxInternalTableItem = ({ type, from, to, value, success, error, gas_limit: gasLimit, created_contract: createdContract, isLoading }: Props) => {
  const typeTitle = TX_INTERNALS_ITEMS.find(({ id }) => id === type)?.title;
  const toData = to ? to : createdContract;

  return (
    <TableRow alignItems="top">
      <TableCell>
        <Flex rowGap={ 2 } flexWrap="wrap">
          { typeTitle && (
            <Box w="126px" display="inline-block">
              <Badge colorPalette="cyan" mr={ 5 } loading={ isLoading }>{ typeTitle }</Badge>
            </Box>
          ) }
          { !success && <TxStatus status="error" errorText={ error } isLoading={ isLoading }/> }
        </Flex>
      </TableCell>
      <TableCell verticalAlign="middle">
        <AddressFromTo
          from={ from }
          to={ toData }
          isLoading={ isLoading }
        />
      </TableCell>
      <TableCell isNumeric verticalAlign="middle">
        <NativeCoinValue
          amount={ value }
          noSymbol
          loading={ isLoading }
        />
      </TableCell>
      <TableCell isNumeric verticalAlign="middle">
        <NativeCoinValue
          amount={ gasLimit }
          units="wei"
          noSymbol
          loading={ isLoading }
        />
      </TableCell>
    </TableRow>
  );
};

export default React.memo(TxInternalTableItem);
