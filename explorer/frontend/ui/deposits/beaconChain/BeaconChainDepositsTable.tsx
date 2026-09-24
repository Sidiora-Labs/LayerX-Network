import React from 'react';

import type { DepositsItem } from 'types/api/deposits';

import config from 'configs/app';
import { AddressHighlightProvider } from 'lib/contexts/addressHighlight';
import useLazyRenderedList from 'lib/hooks/useLazyRenderedList';
import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import BeaconChainDepositsTableItem from './BeaconChainDepositsTableItem';

const feature = config.features.beaconChain;

type Props = {
  top: number;
  isLoading?: boolean;
  items: Array<DepositsItem>;
  view: 'list' | 'address' | 'block';
};

const BeaconChainDepositsTable = ({ items, isLoading, top, view }: Props) => {
  const { cutRef, renderedItemsNum } = useLazyRenderedList(items, !isLoading);

  if (!feature.isEnabled || feature.withdrawalsOnly) {
    return null;
  }

  return (
    <AddressHighlightProvider>
      <TableRoot minW="1100px">
        <TableHeaderSticky top={ top }>
          <TableRow>
            <TableColumnHeader w="190px">Transaction hash</TableColumnHeader>
            { view !== 'block' && <TableColumnHeader>Block</TableColumnHeader> }
            { view !== 'block' && <TableColumnHeader w="180px">Timestamp<TimeFormatToggle/></TableColumnHeader> }
            <TableColumnHeader>{ `Value ${ feature.currency.symbol }` }</TableColumnHeader>
            { view !== 'address' && <TableColumnHeader w="200px">From</TableColumnHeader> }
            <TableColumnHeader>PubKey</TableColumnHeader>
            <TableColumnHeader>Signature</TableColumnHeader>
            <TableColumnHeader>Status</TableColumnHeader>
          </TableRow>
        </TableHeaderSticky>
        <TableBody>
          { items.slice(0, renderedItemsNum).map((item, index) => (
            <BeaconChainDepositsTableItem key={ item.index + (isLoading ? String(index) : '') } item={ item } view={ view } isLoading={ isLoading }/>
          )) }
          <TableRow ref={ cutRef }/>
        </TableBody>
      </TableRoot>
    </AddressHighlightProvider>
  );
};

export default BeaconChainDepositsTable;
