import { Box } from '@chakra-ui/react';
import React from 'react';

import type { TokenTransfer } from 'types/api/tokenTransfer';

import { useMultichainContext } from 'lib/contexts/multichain';
import TokenTransferListItem from 'ui/shared/TokenTransfer/TokenTransferListItem';

interface Props {
  data: Array<TokenTransfer>;
  baseAddress?: string;
  showTxInfo?: boolean;
  enableTimeIncrement?: boolean;
  isLoading?: boolean;
}

const TokenTransferList = ({ data, baseAddress, showTxInfo, enableTimeIncrement, isLoading }: Props) => {
  const multichainContext = useMultichainContext();
  const chainData = multichainContext?.chain;

  return (
    <Box>
      { data.map((item, index) => (
        <TokenTransferListItem
          key={ item.transaction_hash + item.block_hash + item.log_index + (isLoading ? index : '') }
          { ...item }
          baseAddress={ baseAddress }
          showTxInfo={ showTxInfo }
          enableTimeIncrement={ enableTimeIncrement }
          isLoading={ isLoading }
          chainData={ chainData }
        />
      )) }
    </Box>
  );
};

export default React.memo(TokenTransferList);
