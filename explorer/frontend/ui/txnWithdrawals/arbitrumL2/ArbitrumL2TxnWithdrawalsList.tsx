import { Box } from '@chakra-ui/react';
import React from 'react';

import type { ArbitrumL2TxnWithdrawalsItem } from 'types/api/arbitrumL2';

import ArbitrumL2TxnWithdrawalsListItem from './ArbitrumL2TxnWithdrawalsListItem';

interface Props {
  data: Array<ArbitrumL2TxnWithdrawalsItem>;
  isLoading: boolean;
  txHash: string | undefined;
}

const ArbitrumL2TxnWithdrawalsList = ({ data, isLoading, txHash }: Props) => {
  return (
    <Box>
      { data.map((item, index) => (
        <ArbitrumL2TxnWithdrawalsListItem
          key={ String(item.id) + (isLoading ? index : '') }
          data={ item }
          isLoading={ isLoading }
          txHash={ txHash }
        />
      )) }
    </Box>
  );
};

export default React.memo(ArbitrumL2TxnWithdrawalsList);
