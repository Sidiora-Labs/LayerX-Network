import React from 'react';

import type { ClustersLeaderboardObject } from 'types/api/clusters';

import { TableBody, TableHeaderSticky, TableRow, TableColumnHeader, TableRoot } from 'toolkit/chakra/table';

import ClustersLeaderboardTableItem from './ClustersLeaderboardTableItem';

interface Props {
  data: Array<ClustersLeaderboardObject>;
  isLoading?: boolean;
  top?: number;
}

const ClustersLeaderboardTable = ({ data, isLoading, top }: Props) => {
  return (
    <TableRoot>
      <TableHeaderSticky top={ top }>
        <TableRow>
          <TableColumnHeader width="5%">Rank</TableColumnHeader>
          <TableColumnHeader width="40%">Cluster name</TableColumnHeader>
          <TableColumnHeader width="10%">Names</TableColumnHeader>
          <TableColumnHeader width="10%">Total backing</TableColumnHeader>
          <TableColumnHeader width="10%">Active chains</TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { data.map((item, index) => (
          <ClustersLeaderboardTableItem
            key={ `${ item.name }-${ index }${ isLoading ? '-loading' : '' }` }
            item={ item }
            isLoading={ isLoading }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default React.memo(ClustersLeaderboardTable);
