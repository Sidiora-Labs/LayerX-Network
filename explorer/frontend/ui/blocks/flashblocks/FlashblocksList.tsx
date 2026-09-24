import { Box } from '@chakra-ui/react';
import React from 'react';

import type { FlashblockItem } from 'types/client/flashblocks';

import * as SocketNewItemsNotice from 'ui/shared/SocketNewItemsNotice';

import FlashblocksListItem from './FlashblocksListItem';

interface Props {
  data: Array<FlashblockItem>;
  newItemsNum: number | undefined;
  showAlertError?: boolean;
  onAlertLinkClick?: () => void;
}

const FlashblocksList = ({ data, newItemsNum, showAlertError, onAlertLinkClick }: Props) => {
  return (
    <Box>
      { (newItemsNum !== undefined || showAlertError) && (
        <SocketNewItemsNotice.Mobile
          type="flashblock"
          num={ newItemsNum }
          showErrorAlert={ showAlertError }
          onLinkClick={ onAlertLinkClick }
        />
      ) }
      { data.map((item) => (
        <FlashblocksListItem
          key={ `${ item.block_number }-${ item.index }` }
          data={ item }
        />
      )) }
    </Box>
  );
};

export default React.memo(FlashblocksList);
