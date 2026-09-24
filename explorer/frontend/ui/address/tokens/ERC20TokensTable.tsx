import React from 'react';

import type { AddressTokensErc20Item } from './types';

import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';

import ERC20TokensTableItem from './ERC20TokensTableItem';

interface Props {
  data: Array<AddressTokensErc20Item>;
  top: number;
  isLoading: boolean;
  hasAdditionalTokenTypes?: boolean;
}

const ERC20TokensTable = ({ data, top, isLoading, hasAdditionalTokenTypes }: Props) => {
  return (
    <TableRoot>
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader width="30%">Asset</TableColumnHeader>
          <TableColumnHeader width="30%">Contract address</TableColumnHeader>
          <TableColumnHeader width="10%" isNumeric>Price</TableColumnHeader>
          <TableColumnHeader width="15%" isNumeric>Quantity</TableColumnHeader>
          <TableColumnHeader width="15%" isNumeric>Value</TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { data.map((item, index) => (
          <ERC20TokensTableItem
            key={ item.token.address_hash + (isLoading ? index : '') + (item.chain_values ? Object.keys(item.chain_values).join(',') : '') }
            { ...item }
            isLoading={ isLoading }
            hasAdditionalTokenTypes={ hasAdditionalTokenTypes }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default ERC20TokensTable;
