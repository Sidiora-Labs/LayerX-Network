import { Text } from '@chakra-ui/react';
import React from 'react';

import type { PaxeerXBalance } from 'types/api/paxeerX';

import { TableBody, TableColumnHeader, TableHeader, TableRoot, TableRow } from 'toolkit/chakra/table';

import AssetListItem from './AssetListItem';

export interface Props {
  items: Array<PaxeerXBalance>;
  isLoading?: boolean;
}

// One row per asset with a single total; the per-location parts live in an expandable row.
const AssetList = ({ items, isLoading }: Props) => {
  if (items.length === 0) {
    return <Text color="text.secondary">No assets are held by this account.</Text>;
  }

  return (
    <TableRoot data-label="paxeer-x-assets">
      <TableHeader>
        <TableRow>
          <TableColumnHeader width="40%">Asset</TableColumnHeader>
          <TableColumnHeader width="35%">Denom</TableColumnHeader>
          <TableColumnHeader width="25%" isNumeric>Total</TableColumnHeader>
        </TableRow>
      </TableHeader>
      <TableBody>
        { items.map((item) => (
          <AssetListItem key={ item.asset.id } item={ item } isLoading={ isLoading }/>
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default React.memo(AssetList);
