import type { GasPriceInfo } from 'types/api/stats';
import type { GasUnit } from 'types/client/gasTracker';

import { currencyUnits } from 'lib/units';

export default function formatGasValue(data: GasPriceInfo, unit: GasUnit) {
  switch (unit) {
    case 'gwei': {
      if (!data.price) {
        return `N/A ${ currencyUnits.gwei }`;
      }

      if (Number(data.price) < 0.1) {
        return `< 0.1 ${ currencyUnits.gwei }`;
      }

      return `${ Number(data.price).toLocaleString(undefined, { maximumFractionDigits: 1 }) } ${ currencyUnits.gwei }`;
    }

    case 'usd': {
      if (!data.fiat_price) {
        return `$N/A`;
      }

      if (Number(data.fiat_price) < 0.01) {
        return `< $0.01`;
      }

      return `$${ Number(data.fiat_price).toLocaleString(undefined, { maximumFractionDigits: 2 }) }`;
    }
  }
}
