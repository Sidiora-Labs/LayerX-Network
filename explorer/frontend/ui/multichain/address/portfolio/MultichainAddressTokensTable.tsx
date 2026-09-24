import React from 'react';

import type { AddressTokensErc20Item } from 'ui/address/tokens/types';

import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';

import MultichainAddressTokensTableItem from './MultichainAddressTokensTableItem';

interface Props {
  data: Array<AddressTokensErc20Item>;
  top: number;
  isLoading: boolean;
}

const MultichainAddressTokensTable = ({ data, top, isLoading }: Props) => {
  return (
    <TableRoot>
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader width="55%">Asset</TableColumnHeader>
          <TableColumnHeader width="15%" isNumeric>Price</TableColumnHeader>
          <TableColumnHeader width="15%" isNumeric>Quantity</TableColumnHeader>
          <TableColumnHeader width="15%" isNumeric>Value</TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { data.map((item, index) => (
          <MultichainAddressTokensTableItem
            key={ item.token.address_hash + (isLoading ? index : '') + (item.chain_values ? Object.keys(item.chain_values).join(',') : '') }
            data={ item }
            isLoading={ isLoading }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default React.memo(MultichainAddressTokensTable);
