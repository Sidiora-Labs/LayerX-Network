import type { Feature } from './types';
import { GAS_UNITS } from 'types/client/gasTracker';
import type { GasUnit } from 'types/client/gasTracker';

import chainConfig from '../chain';
import { getEnvValue, parseEnvJson } from '../utils';

const isDisabled = getEnvValue('NEXT_PUBLIC_GAS_TRACKER_ENABLED') === 'false';

const units = ((): Array<GasUnit> => {
  const envValue = getEnvValue('NEXT_PUBLIC_GAS_TRACKER_UNITS');
  if (!envValue) {
    if (chainConfig.isTestnet) {
      return [ 'gwei' ];
    }
    return [ 'usd', 'gwei' ];
  }

  const units = parseEnvJson<Array<GasUnit>>(envValue)?.filter((type) => GAS_UNITS.includes(type)) || [];

  return units;
})();

const title = 'Gas tracker';

const config: Feature<{ units: Array<GasUnit> }> = (() => {
  if (!isDisabled && units.length > 0) {
    return Object.freeze({
      title,
      isEnabled: true,
      units,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
