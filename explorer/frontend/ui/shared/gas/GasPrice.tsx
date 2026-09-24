import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { GasPriceInfo } from 'types/api/stats';
import type { GasUnit } from 'types/client/gasTracker';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';

import formatGasValue from './formatGasValue';

const UNITS_TO_API_FIELD_MAP: Record<GasUnit, 'price' | 'fiat_price'> = {
  gwei: 'price',
  usd: 'fiat_price',
};

interface Props {
  data: GasPriceInfo | null;
  className?: string;
  unitMode?: 'primary' | 'secondary';
  prefix?: string;
}

const GasPrice = ({ data, prefix, className, unitMode = 'primary' }: Props) => {
  const multichainContext = useMultichainContext();
  const feature = multichainContext?.chain?.app_config.features.gasTracker || config.features.gasTracker;

  if (!data || !feature.isEnabled) {
    return null;
  }

  switch (unitMode) {
    case 'secondary': {
      const primaryUnits = feature.units[0];
      const secondaryUnits = feature.units[1];

      if (!secondaryUnits) {
        return null;
      }

      const primaryUnitsValue = data[UNITS_TO_API_FIELD_MAP[primaryUnits]];
      if (!primaryUnitsValue) {
        // in this case we display values in secondary units in primary mode as fallback
        return null;
      }

      const secondaryUnitsValue = data[UNITS_TO_API_FIELD_MAP[secondaryUnits]];
      if (!secondaryUnitsValue) {
        return null;
      }

      const formattedValue = formatGasValue(data, secondaryUnits);
      return <span className={ className }>{ prefix }{ formattedValue }</span>;
    }
    case 'primary': {
      const primaryUnits = feature.units[0];
      const secondaryUnits = feature.units[1];

      if (!primaryUnits) {
        // this should never happen since feature will be disabled if there are no units at all
        return null;
      }

      const value = data[UNITS_TO_API_FIELD_MAP[primaryUnits]];
      if (!value) {
        // in primary mode we want to fallback to secondary units if value in primary units are not available
        // unless there are no secondary units
        const valueInSecondaryUnits = data[UNITS_TO_API_FIELD_MAP[secondaryUnits]];

        if (!secondaryUnits || !valueInSecondaryUnits) {
          // in primary mode we always want to show something
          // this will return "N/A <units>"
          return <span className={ className }>{ formatGasValue(data, primaryUnits) }</span>;
        } else {
          return <span className={ className }>{ prefix }{ formatGasValue(data, secondaryUnits) }</span>;
        }
      }

      return <span className={ className }>{ prefix }{ formatGasValue(data, primaryUnits) }</span>;
    }
  }
};

export default chakra(GasPrice);
