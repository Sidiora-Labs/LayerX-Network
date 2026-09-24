import React from 'react';

import type { FlashblockItem } from 'types/client/flashblocks';

import { TableBody, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import * as SocketNewItemsNotice from 'ui/shared/SocketNewItemsNotice';

import FlashblocksTableItem from './FlashblocksTableItem';

interface Props {
  items: Array<FlashblockItem>;
  newItemsNum: number | undefined;
  showAlertError?: boolean;
  onAlertLinkClick?: () => void;
}

const FlashblocksTable = ({ items, newItemsNum, showAlertError, onAlertLinkClick }: Props) => {
  return (
    <TableRoot>
      <TableHeaderSticky top={ 0 }>
        <TableRow>
          <TableColumnHeader width="50%">
            Block
          </TableColumnHeader>
          <TableColumnHeader width="20%" isNumeric>
            Txn
          </TableColumnHeader>
          <TableColumnHeader width="30%" isNumeric>
            Gas used
          </TableColumnHeader>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { (newItemsNum !== undefined || showAlertError) && (
          <SocketNewItemsNotice.Desktop
            type="flashblock"
            num={ newItemsNum }
            showErrorAlert={ showAlertError }
            onLinkClick={ onAlertLinkClick }
          />
        ) }
        { items.map((item) => (
          <FlashblocksTableItem
            key={ `${ item.block_number }-${ item.index }` }
            data={ item }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default React.memo(FlashblocksTable);
