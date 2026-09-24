import BigNumber from 'bignumber.js';
import React from 'react';

import type { Transaction } from 'types/api/transaction';

import config from 'configs/app';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import * as DetailedInfoItemBreakdown from 'ui/shared/DetailedInfo/DetailedInfoItemBreakdown';
import TxFee from 'ui/shared/tx/TxFee';
import NativeCoinValue from 'ui/shared/value/NativeCoinValue';

interface Props {
  isLoading: boolean;
  data: Transaction;
}

const TxDetailsTxFee = ({ isLoading, data }: Props) => {

  if (config.UI.views.tx.hiddenFields?.tx_fee) {
    return null;
  }

  const content = (() => {
    if (!config.UI.views.tx.groupedFees) {
      return (
        <TxFee
          tx={ data }
          loading={ isLoading }
          accuracy={ 0 }
          rowGap={ 0 }
          hasExchangeRateToggle
        />
      );
    }

    const exchangeRate = 'exchange_rate' in data ? data.exchange_rate : null;
    const historicalExchangeRate = 'historic_exchange_rate' in data ? data.historic_exchange_rate : null;

    return (
      <>
        <NativeCoinValue
          amount={ data.fee.value }
          exchangeRate={ exchangeRate }
          historicalExchangeRate={ historicalExchangeRate }
          hasExchangeRateToggle
          loading={ isLoading }
          unitsTooltip="gwei"
          copyOriginalValue
          accuracy={ 0 }
          flexWrap="wrap"
          mr={ 3 }
          rowGap={ 0 }
        />
        <DetailedInfoItemBreakdown.Container loading={ isLoading }>
          <DetailedInfoItemBreakdown.Row
            label="Base fee"
            hint="The minimum network fee charged per transaction"
          >
            <NativeCoinValue
              amount={ BigNumber(data.base_fee_per_gas || 0).multipliedBy(data.gas_used || 0).toString() }
              exchangeRate={ exchangeRate }
              historicalExchangeRate={ historicalExchangeRate }
              hasExchangeRateToggle
              accuracy={ 0 }
              unitsTooltip="gwei"
              copyOriginalValue
              loading={ isLoading }
              flexWrap="wrap"
              rowGap={ 0 }
            />
          </DetailedInfoItemBreakdown.Row>
          <DetailedInfoItemBreakdown.Row
            label="Priority fee"
            hint="An extra fee set by the sender to speed up transaction execution"
          >
            <NativeCoinValue
              amount={ data.priority_fee || '0' }
              exchangeRate={ exchangeRate }
              historicalExchangeRate={ historicalExchangeRate }
              hasExchangeRateToggle
              accuracy={ 0 }
              unitsTooltip="gwei"
              copyOriginalValue
              loading={ isLoading }
              flexWrap="wrap"
              rowGap={ 0 }
            />
          </DetailedInfoItemBreakdown.Row>
        </DetailedInfoItemBreakdown.Container>

      </>
    );
  })();

  return (
    <>
      <DetailedInfo.ItemLabel
        hint={ data.blob_gas_used ? 'Transaction fee without blob fee' : 'Total transaction fee' }
        isLoading={ isLoading }
      >
        Transaction fee
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue multiRow>
        { content }
      </DetailedInfo.ItemValue>
    </>
  );
};

export default React.memo(TxDetailsTxFee);
