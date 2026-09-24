import React from 'react';

import type { TokenTransfer } from 'types/api/tokenTransfer';

import { AddressHighlightProvider } from 'lib/contexts/addressHighlight';
import { useMultichainContext } from 'lib/contexts/multichain';
import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import * as SocketNewItemsNotice from 'ui/shared/SocketNewItemsNotice';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';
import TokenTransferTableItem from 'ui/shared/TokenTransfer/TokenTransferTableItem';

interface Props {
  data: Array<TokenTransfer>;
  baseAddress?: string;
  showTxInfo?: boolean;
  top: number;
  enableTimeIncrement?: boolean;
  showSocketInfo?: boolean;
  showSocketErrorAlert?: boolean;
  socketInfoNum?: number;
  isLoading?: boolean;
}

const TokenTransferTable = ({
  data,
  baseAddress,
  showTxInfo,
  top,
  enableTimeIncrement,
  showSocketInfo,
  showSocketErrorAlert,
  socketInfoNum,
  isLoading,
}: Props) => {
  const multichainContext = useMultichainContext();
  const chainData = multichainContext?.chain;

  return (
    <AddressHighlightProvider>
      <TableRoot minW={ chainData ? '1000px' : '950px' }>
        <TableHeaderSticky top={ top }>
          <TableRow>
            { showTxInfo && <TableColumnHeader width="48px"></TableColumnHeader> }
            { chainData && <TableColumnHeader width={ showTxInfo ? '32px' : '38px' }/> }
            <TableColumnHeader width="230px">Token</TableColumnHeader>
            <TableColumnHeader width="160px">Token ID</TableColumnHeader>
            { showTxInfo && (
              <TableColumnHeader width="200px">
                Txn hash
                <TimeFormatToggle/>
              </TableColumnHeader>
            ) }
            <TableColumnHeader width="60%">From/To</TableColumnHeader>
            <TableColumnHeader width="40%" isNumeric>Value</TableColumnHeader>
          </TableRow>
        </TableHeaderSticky>
        <TableBody>
          { showSocketInfo && (
            <SocketNewItemsNotice.Desktop
              showErrorAlert={ showSocketErrorAlert }
              num={ socketInfoNum }
              type="token_transfer"
              isLoading={ isLoading }
            />
          ) }
          { data.map((item, index) => (
            <TokenTransferTableItem
              key={ item.transaction_hash + item.block_hash + item.log_index + (isLoading ? index : '') }
              { ...item }
              baseAddress={ baseAddress }
              showTxInfo={ showTxInfo }
              enableTimeIncrement={ enableTimeIncrement }
              isLoading={ isLoading }
              chainData={ chainData }
            />
          )) }
        </TableBody>
      </TableRoot>
    </AddressHighlightProvider>
  );
};

export default React.memo(TokenTransferTable);
