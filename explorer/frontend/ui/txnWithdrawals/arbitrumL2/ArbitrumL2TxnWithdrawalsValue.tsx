import React from 'react';

import type { ArbitrumL2TxnWithdrawalsItem } from 'types/api/arbitrumL2';
import type { TokenInfo } from 'types/api/token';

import { Skeleton } from 'toolkit/chakra/skeleton';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';
import TokenValue from 'ui/shared/value/TokenValue';

interface Props {
  data: ArbitrumL2TxnWithdrawalsItem;
  loading?: boolean;
}

const ArbitrumL2TxnWithdrawalsValue = ({ data, loading }: Props) => {

  if (data.token) {
    const token: TokenInfo | null = {
      ...data.token,
      decimals: String(data.token.decimals),
      type: 'ERC-20',
      holders_count: null,
      exchange_rate: null,
      total_supply: null,
      circulating_market_cap: null,
      icon_url: null,
      reputation: null,
    };

    return (
      <TokenValue
        amount={ data.token.amount ?? '0' }
        token={ token }
        layer="L1"
        tokenEntityProps={{ noIcon: true }}
        loading={ loading }
      />
    );
  }

  if (data.callvalue && data.callvalue !== '0') {
    return (
      <NativeCoinValue
        amount={ data.callvalue }
        loading={ loading }
      />
    );
  }

  return <Skeleton loading={ loading }><span>-</span></Skeleton>;
};

export default React.memo(ArbitrumL2TxnWithdrawalsValue);
