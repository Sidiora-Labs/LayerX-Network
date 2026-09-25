import type { BoxProps } from '@chakra-ui/react';
import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { TokenInfo } from 'types/api/token';
import type { Transaction, WrappedTransactionFields } from 'types/api/transaction';

import config from 'configs/app';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';
import TokenValue from 'ui/shared/value/TokenValue';

interface Props extends BoxProps {
  loading?: boolean;
  tx: Transaction | Pick<Transaction, WrappedTransactionFields>;
  hasExchangeRateToggle?: boolean;
  accuracy?: number;
  accuracyUsd?: number;
  noTooltip?: boolean;
  noSymbol?: boolean;
  noUsd?: boolean;
  layout?: 'horizontal' | 'vertical';
}

export const isNonNativeFeeToken = (token: Transaction['fee']['token']): token is TokenInfo => {
  return Boolean(token?.symbol && token.decimals != null && (
    token.symbol !== config.chain.currency.symbol || Number(token.decimals) !== config.chain.currency.decimals
  ));
};

const TxFee = ({ tx, accuracy, accuracyUsd, loading, noSymbol: noSymbolProp, noUsd, noTooltip, hasExchangeRateToggle, ...rest }: Props) => {

  if (isNonNativeFeeToken(tx.fee.token)) {
    return (
      <TokenValue
        amount={ tx.fee.value || '0' }
        token={ tx.fee.token }
        exchangeRate={ noUsd ? null : tx.fee.token.exchange_rate }
        accuracy={ accuracy }
        accuracyUsd={ accuracyUsd }
        loading={ loading }
        noTooltip={ noTooltip }
        endElement={ noSymbolProp || config.UI.views.tx.hiddenFields?.fee_currency ? '' : undefined }
        { ...rest }
      />
    );
  }

  if ('celo' in tx && tx.celo?.gas_token) {
    return (
      <TokenValue
        amount={ tx.fee.value || '0' }
        token={ tx.celo.gas_token }
        accuracy={ accuracy }
        accuracyUsd={ accuracyUsd }
        loading={ loading }
        { ...rest }
      />
    );
  }

  if ('stability_fee' in tx && tx.stability_fee) {
    return (
      <TokenValue
        amount={ tx.stability_fee.total_fee }
        token={ tx.stability_fee.token }
        accuracy={ accuracy }
        accuracyUsd={ accuracyUsd }
        loading={ loading }
        { ...rest }
      />
    );
  }

  const noSymbol = noSymbolProp || config.UI.views.tx.hiddenFields?.fee_currency;
  const exchangeRate = 'exchange_rate' in tx ? tx.exchange_rate : null;
  const historicalExchangeRate = 'historic_exchange_rate' in tx ? tx.historic_exchange_rate : null;

  return (
    <NativeCoinValue
      amount={ tx.fee.value || '0' }
      noSymbol={ noSymbol }
      exchangeRate={ noUsd ? null : exchangeRate }
      historicalExchangeRate={ noUsd ? null : historicalExchangeRate }
      hasExchangeRateToggle={ hasExchangeRateToggle }
      accuracy={ accuracy }
      accuracyUsd={ accuracyUsd }
      loading={ loading }
      noTooltip={ noTooltip }
      flexWrap="wrap"
      { ...rest }
    />
  );
};

export default React.memo(chakra(TxFee));
