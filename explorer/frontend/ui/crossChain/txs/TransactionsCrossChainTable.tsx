import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { InterchainMessage } from '@blockscout/interchain-indexer-types';

import { AddressHighlightProvider } from 'lib/contexts/addressHighlight';
import { TableBody, TableColumnHeader, TableHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import TransactionsCrossChainTableItem from './TransactionsCrossChainTableItem';

interface Props {
  data: Array<InterchainMessage>;
  isLoading?: boolean;
  top?: number;
  stickyHeader?: boolean;
  currentAddress?: string;
}

const TransactionsCrossChainTable = ({ data, isLoading, top, stickyHeader, currentAddress }: Props) => {
  const TableHeaderComponent = stickyHeader ? TableHeaderSticky : TableHeader;

  return (
    <AddressHighlightProvider>
      <TableRoot tableLayout="auto">
        <TableHeaderComponent top={ stickyHeader ? top : undefined }>
          <TableRow>
            <TableColumnHeader w="42px"/>
            { currentAddress && <TableColumnHeader w="44px"/> }
            <TableColumnHeader>Message</TableColumnHeader>
            <TableColumnHeader>
              <Flex alignItems="center" flexWrap="nowrap">
                Timestamp
                <TimeFormatToggle/>
              </Flex>
            </TableColumnHeader>
            <TableColumnHeader>Msg sender</TableColumnHeader>
            <TableColumnHeader>Source tx</TableColumnHeader>
            <TableColumnHeader>Dest tx</TableColumnHeader>
            <TableColumnHeader>Transf</TableColumnHeader>
            <TableColumnHeader>Sender</TableColumnHeader>
            <TableColumnHeader/>
            <TableColumnHeader>Recipient</TableColumnHeader>
            <TableColumnHeader>Protocol</TableColumnHeader>
          </TableRow>
        </TableHeaderComponent>
        <TableBody>
          { data.map((item, index) => (
            <TransactionsCrossChainTableItem
              key={ item.message_id + (isLoading ? String(index) : '') }
              data={ item }
              isLoading={ isLoading }
              currentAddress={ currentAddress }
            />
          )) }
        </TableBody>
      </TableRoot>
    </AddressHighlightProvider>
  );
};

export default React.memo(TransactionsCrossChainTable);
