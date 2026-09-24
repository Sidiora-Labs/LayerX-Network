import { Text, chakra } from '@chakra-ui/react';
import React from 'react';

import { CoinType } from '@blockscout/zetachain-cctx-types';

import config from 'configs/app';
import { Skeleton } from 'toolkit/chakra/skeleton';
import calculateUsdValue from 'ui/shared/value/calculateUsdValue';

type Props = {
  coinType: CoinType;
  tokenSymbol?: string;
  amount: string;
  decimals?: number | null;
  isLoading?: boolean;
  className?: string;
  accuracy?: number;
};

const ZetaChainCCTXValue = ({ coinType, tokenSymbol, amount, decimals, isLoading, className, accuracy = 8 }: Props) => {
  let unit: string;
  let value: string | undefined;
  switch (coinType) {
    case CoinType.ERC20:
      unit = tokenSymbol || 'Unnamed token';
      value = calculateUsdValue({ amount, decimals: decimals?.toString() || '18', accuracy }).valueStr;
      break;
    case CoinType.ZETA:
      unit = config.chain.currency.symbol || config.chain.currency.name || '';
      value = calculateUsdValue({ amount, decimals: config.chain.currency.decimals?.toString() || '18', accuracy }).valueStr;
      break;
    case CoinType.GAS:
      unit = tokenSymbol || 'Unnamed token';
      value = calculateUsdValue({ amount, decimals: decimals?.toString() || '18', accuracy }).valueStr;
      break;
    default:
      unit = '-';
      break;
  }

  return (
    <Skeleton loading={ isLoading } display="flex" gap={ 1 } overflow="hidden" className={ className }>
      <Text overflow="hidden" whiteSpace="nowrap" textOverflow="ellipsis">{ value }</Text>
      <Text color="text.secondary" whiteSpace="nowrap" overflow="hidden" textOverflow="ellipsis">{ unit }</Text>
    </Skeleton>
  );
};

export default React.memo(chakra(ZetaChainCCTXValue));
