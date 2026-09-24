import BigNumber from 'bignumber.js';
import React from 'react';

import config from 'configs/app';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoNativeCoinValue from 'ui/shared/DetailedInfo/DetailedInfoNativeCoinValue';

interface Props {
  txFee: string | null;
  gasUsed: string | null;
  isLoading?: boolean;
}

const TxDetailsFeePerGas = ({ txFee, gasUsed, isLoading }: Props) => {
  if (!config.UI.views.tx.additionalFields?.fee_per_gas || !gasUsed || txFee === null) {
    return null;
  }

  return (
    <>
      <DetailedInfo.ItemLabel
        hint="Fee per gas"
        isLoading={ isLoading }
      >
        Fee per gas
      </DetailedInfo.ItemLabel>
      <DetailedInfoNativeCoinValue
        amount={ BigNumber(txFee).dividedBy(gasUsed).toFixed() }
        noSymbol={ config.UI.views.tx.hiddenFields?.fee_currency }
        loading={ isLoading }
      />
    </>
  );
};

export default TxDetailsFeePerGas;
