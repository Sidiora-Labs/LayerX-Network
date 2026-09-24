import React from 'react';

import type { TokenInfo } from 'types/api/token';

import config from 'configs/app';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import GasPriceValue from 'ui/shared/value/GasPriceValue';
import TokenValue from 'ui/shared/value/TokenValue';

interface Props {
  gasToken?: TokenInfo | null;
  gasPrice: string | null;
  isLoading?: boolean;
}

const TxDetailsGasPrice = ({ gasPrice, gasToken, isLoading }: Props) => {
  if (config.UI.views.tx.hiddenFields?.gas_price || !gasPrice) {
    return null;
  }

  const content = (() => {
    if (gasToken) {
      return (
        <TokenValue
          amount={ gasPrice }
          token={ gasToken }
          loading={ isLoading }
          accuracy={ 0 }
        />
      );
    }

    return (
      <GasPriceValue
        amount={ gasPrice }
        loading={ isLoading }
      />
    );
  })();

  return (
    <>
      <DetailedInfo.ItemLabel
        hint="Price per unit of gas specified by the sender. Higher gas prices can prioritize transaction inclusion during times of high usage"
        isLoading={ isLoading }
      >
        Gas price
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue multiRow>
        { content }
      </DetailedInfo.ItemValue>
    </>
  );
};

export default TxDetailsGasPrice;
