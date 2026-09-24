import { Flex, chakra } from '@chakra-ui/react';
import React from 'react';

import type { CctxListItem } from '@blockscout/zetachain-cctx-types';
import type { ZetaChainCCTXFilterParams } from 'types/client/zetaChain';

import { AddressHighlightProvider } from 'lib/contexts/addressHighlight';
import useInitialList from 'lib/hooks/useInitialList';
import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import * as SocketNewItemsNotice from 'ui/shared/SocketNewItemsNotice';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import ZetaChainFilterByColumn from '../filters/ZetaChainFilterByColumn';
import ZetaChainCCTxsTableItem from './ZetaChainCCTxsTableItem';

type Props = {
  txs: Array<CctxListItem>;
  top: number;
  enableTimeIncrement?: boolean;
  isLoading?: boolean;
  filters?: ZetaChainCCTXFilterParams;
  onFilterChange: <T extends keyof ZetaChainCCTXFilterParams>(field: T, val: ZetaChainCCTXFilterParams[T]) => void;
  isPlaceholderData?: boolean;
  showStatusFilter?: boolean;
  showSocketInfo?: boolean;
  showSocketErrorAlert?: boolean;
  socketInfoNum?: number;
};

const ZetaChainCCTxsTable = ({
  txs,
  top,
  enableTimeIncrement,
  isLoading,
  filters = {},
  onFilterChange,
  isPlaceholderData,
  showStatusFilter = true,
  showSocketInfo = false,
  showSocketErrorAlert = false,
  socketInfoNum = 0,
}: Props) => {
  const initialList = useInitialList({
    data: txs ?? [],
    idFn: (item) => item.index,
    enabled: !isLoading,
  });

  return (
    <AddressHighlightProvider>
      <TableRoot minWidth="950px">
        <TableHeaderSticky top={ top }>
          <TableRow>
            <TableColumnHeader width="300px">
              <Flex alignItems="center" columnGap={ 2 }>
                <chakra.span lineHeight="24px" verticalAlign="middle">
                  CCTx hash
                </chakra.span>
                <ZetaChainFilterByColumn
                  column="age"
                  columnName="Age"
                  filters={ filters }
                  handleFilterChange={ onFilterChange }
                  isLoading={ isPlaceholderData }
                />
                <TimeFormatToggle ml={ 0 }/>
              </Flex>
            </TableColumnHeader>
            <TableColumnHeader width="105px">
              <chakra.span mr={ 2 } lineHeight="24px" verticalAlign="middle">
                Status
              </chakra.span>
              { showStatusFilter && (
                <ZetaChainFilterByColumn
                  column="status"
                  columnName="Status"
                  filters={ filters }
                  handleFilterChange={ onFilterChange }
                  isLoading={ isPlaceholderData }
                />
              ) }
            </TableColumnHeader>
            <TableColumnHeader width="200px">
              <chakra.span mr={ 2 } lineHeight="24px" verticalAlign="middle">
                Sender
              </chakra.span>
              <ZetaChainFilterByColumn
                column="sender"
                columnName="Sender"
                filters={ filters }
                handleFilterChange={ onFilterChange }
                isLoading={ isPlaceholderData }
              />
            </TableColumnHeader>
            <TableColumnHeader width="165px">
              <chakra.span mr={ 2 } lineHeight="24px" verticalAlign="middle">
                Receiver
              </chakra.span>
              <ZetaChainFilterByColumn
                column="receiver"
                columnName="Receiver"
                filters={ filters }
                handleFilterChange={ onFilterChange }
                isLoading={ isPlaceholderData }
              />
            </TableColumnHeader>
            <TableColumnHeader width="100%" isNumeric>
              <chakra.span mr={ 2 } lineHeight="24px" verticalAlign="middle">
                Value
              </chakra.span>
              <ZetaChainFilterByColumn
                column="asset"
                columnName="Asset"
                filters={ filters }
                handleFilterChange={ onFilterChange }
                isLoading={ isPlaceholderData }
              />
            </TableColumnHeader>
          </TableRow>
        </TableHeaderSticky>
        <TableBody>
          { showSocketInfo && (
            <SocketNewItemsNotice.Desktop
              showErrorAlert={ showSocketErrorAlert }
              num={ socketInfoNum }
              type="cross_chain_transaction"
              isLoading={ isLoading }
            />
          ) }
          { txs.map((item, index) => (
            <ZetaChainCCTxsTableItem
              key={ item.index + (isLoading ? index : '') }
              tx={ item }
              enableTimeIncrement={ enableTimeIncrement }
              isLoading={ isLoading }
              animation={ initialList.getAnimationProp(item) }
            />
          )) }
        </TableBody>
      </TableRoot>
    </AddressHighlightProvider>
  );
};

export default React.memo(ZetaChainCCTxsTable);
