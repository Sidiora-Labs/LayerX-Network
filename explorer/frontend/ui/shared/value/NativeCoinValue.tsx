import { Box } from '@chakra-ui/react';
import BigNumber from 'bignumber.js';
import React from 'react';

import { thinsp } from 'toolkit/utils/htmlEntities';
import CopyToClipboard from 'ui/shared/CopyToClipboard';

import type { Props as AssetValueProps } from './AssetValue';
import AssetValue from './AssetValue';
import type { Unit } from './utils';
import { GWEI, getUnitName, getUnitDecimals } from './utils';

export interface Props extends AssetValueProps {
  units?: Unit;
  unitsTooltip?: Unit;
  // if units in tooltip are different from units and copyOriginalValue is true, the original value will be copied to clipboard
  // otherwise the the value shown in tooltip will be copied
  copyOriginalValue?: boolean;
  noSymbol?: boolean;
  // if value is greater than 10 ^ gweiThreshold and unit are wei, gwei units will be used for better formatting
  gweiThreshold?: number;
}

const NativeCoinValue = ({
  amount,
  asset: assetProp,
  units: unitsProp = 'ether',
  unitsTooltip,
  copyOriginalValue,
  noSymbol,
  loading,
  gweiThreshold,
  accuracy,
  noTooltip,
  ...rest
}: Props) => {

  const units = React.useMemo(() => {
    if (amount && gweiThreshold && unitsProp === 'wei') {
      const valueInGwei = BigNumber(amount).div(GWEI);
      if (valueInGwei.isGreaterThanOrEqualTo(10 ** -gweiThreshold)) {
        return 'gwei';
      }
    }

    return unitsProp;
  }, [ gweiThreshold, unitsProp, amount ]);

  const decimals = getUnitDecimals(units);

  const asset = (() => {
    if (!noSymbol) {
      if (assetProp) {
        return assetProp;
      }

      return getUnitName(units);
    }
  })();

  const tooltipContent = React.useMemo(() => {
    if (unitsTooltip && unitsTooltip !== units) {
      const unitDecimals = getUnitDecimals(unitsTooltip);
      const unitName = getUnitName(unitsTooltip);
      const displayedValue = BigNumber(amount || 0).div(new BigNumber(10).pow(unitDecimals));
      const valueToCopy = copyOriginalValue ? BigNumber(amount || 0).div(new BigNumber(10).pow(decimals)).toFixed() : displayedValue.toFixed();

      return (
        <Box display="inline" whiteSpace="wrap" wordBreak="break-all">
          { displayedValue.toFormat() }{ thinsp }{ unitName }
          <CopyToClipboard text={ valueToCopy } verticalAlign="bottom" noTooltip/>
        </Box>
      );
    }
  }, [ unitsTooltip, units, amount, copyOriginalValue, decimals ]);

  return (
    <AssetValue
      amount={ amount }
      decimals={ decimals }
      asset={ asset }
      loading={ loading }
      accuracy={ unitsProp === 'wei' && units === 'gwei' && gweiThreshold ? gweiThreshold : accuracy }
      tooltipContent={ tooltipContent }
      noTooltip={ tooltipContent ? false : noTooltip }
      { ...rest }
    />
  );
};

export default React.memo(NativeCoinValue);
