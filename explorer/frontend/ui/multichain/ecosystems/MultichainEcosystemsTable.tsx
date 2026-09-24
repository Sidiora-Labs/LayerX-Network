import { chakra } from '@chakra-ui/react';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';
import type { ChainMetricsSortingField, ChainMetricsSortingValue } from 'types/client/multichainAggregator';

import multichainConfig from 'configs/multichain';
import { MultichainProvider } from 'lib/contexts/multichain';
import { TableBody, TableColumnHeader, TableColumnHeaderSortable, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import getNextSortValue from 'ui/shared/sort/getNextSortValue';

import MultichainEcosystemsTableItem from './MultichainEcosystemsTableItem';
import { SORT_SEQUENCE } from './utils';

interface Props {
  data: Array<multichain.ChainMetrics>;
  sort: ChainMetricsSortingValue;
  setSorting: ({ value }: { value: Array<string> }) => void;
  isLoading?: boolean;
}

const MultichainEcosystemsTable = ({ data, isLoading, sort, setSorting }: Props) => {

  const chains = multichainConfig()?.chains;

  const onSortToggle = React.useCallback((field: ChainMetricsSortingField) => {
    const value = getNextSortValue<ChainMetricsSortingField, ChainMetricsSortingValue>(SORT_SEQUENCE, field)(sort);
    setSorting({ value: [ value ] });
  }, [ sort, setSorting ]);

  return (
    <TableRoot minW="1000px">
      <TableHeaderSticky>
        <TableRow>
          <TableColumnHeader width="30%">Chain name</TableColumnHeader>
          <TableColumnHeaderSortable width="17.5%" sortField="active_accounts" sortValue={ sort } onSortToggle={ onSortToggle }>
            Active addresses
            <chakra.span color="text.secondary" whiteSpace="pre"> 7D</chakra.span>
          </TableColumnHeaderSortable>
          <TableColumnHeaderSortable width="17.5%" sortField="new_addresses" sortValue={ sort } onSortToggle={ onSortToggle }>
            New addresses
            <chakra.span color="text.secondary" whiteSpace="pre"> 7D</chakra.span>
          </TableColumnHeaderSortable>
          <TableColumnHeaderSortable width="17.5%" sortField="daily_transactions" sortValue={ sort } onSortToggle={ onSortToggle }>
            Daily txs
            <chakra.span color="text.secondary" whiteSpace="pre"> 7D</chakra.span>
          </TableColumnHeaderSortable>
          <TableColumnHeaderSortable width="17.5%" sortField="tps" sortValue={ sort } onSortToggle={ onSortToggle }>
            TPS
          </TableColumnHeaderSortable>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { data.map((item, index) => (
          <MultichainProvider key={ item.chain_id + (isLoading ? `_${ index }` : '') } chainId={ item.chain_id }>
            <MultichainEcosystemsTableItem
              data={ item }
              isLoading={ isLoading }
              chainInfo={ chains?.find((chain) => chain.id === item.chain_id) }
            />
          </MultichainProvider>
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default React.memo(MultichainEcosystemsTable);
