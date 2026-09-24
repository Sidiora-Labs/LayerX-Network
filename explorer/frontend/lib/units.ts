import config from 'configs/app';
import type { Unit } from 'ui/shared/value/utils';

const weiName = config.chain.currency.weiName || 'wei';
const gweiName = config.chain.currency.gweiName || `G${ weiName }`;

export const currencyUnits: Record<Unit, string> = {
  wei: weiName,
  gwei: gweiName,
  ether: config.chain.currency.symbol || 'ETH',
};
