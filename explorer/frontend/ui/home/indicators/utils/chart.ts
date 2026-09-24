import type { TimeChartData, TimeChartDataItem, TimeChartItemRaw, TimeChartItem } from 'toolkit/components/charts/types';
import type { ChainIndicatorId } from 'types/homepage';

import config from 'configs/app';
import { sortByDateDesc } from 'ui/shared/chart/utils';

const CHART_ITEMS: Record<ChainIndicatorId, Pick<TimeChartDataItem, 'name' | 'valueFormatter'>> = {
  daily_txs: {
    name: 'Tx/day',
    valueFormatter: (x: number) => x.toLocaleString(undefined, { maximumFractionDigits: 2, notation: 'compact' }),
  },
  daily_operational_txs: {
    name: 'Tx/day',
    valueFormatter: (x: number) => x.toLocaleString(undefined, { maximumFractionDigits: 2, notation: 'compact' }),
  },
  coin_price: {
    name: `${ config.features.multichain.isEnabled ? 'ETH' : config.chain.currency.symbol } price`,
    valueFormatter: (x: number) => '$' + x.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 6 }),
  },
  secondary_coin_price: {
    name: `${ config.chain.currency.symbol } price`,
    valueFormatter: (x: number) => '$' + x.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 6 }),
  },
  market_cap: {
    name: 'Market cap',
    valueFormatter: (x: number) => '$' + x.toLocaleString(undefined, { maximumFractionDigits: 2 }),
  },
  tvl: {
    name: 'TVL',
    valueFormatter: (x: number) => '$' + x.toLocaleString(undefined, { maximumFractionDigits: 2, notation: 'compact' }),
  },
};

const nonNullTailReducer = (result: Array<TimeChartItemRaw>, item: TimeChartItemRaw) => {
  if (item.value === null && result.length === 0) {
    return result;
  }
  result.unshift(item);
  return result;
};

const mapNullToZero: (item: TimeChartItemRaw) => TimeChartItem = (item) => ({ ...item, value: Number(item.value) });

export function prepareChartItems(items: Array<TimeChartItemRaw>) {
  return items
    .sort(sortByDateDesc)
    .reduceRight(nonNullTailReducer, [] as Array<TimeChartItemRaw>)
    .map(mapNullToZero);
}

export function getChartData(indicatorId: ChainIndicatorId, data: Array<TimeChartItemRaw>): TimeChartData {
  return [ {
    id: indicatorId.replace(' ', '_'),
    charts: [],
    items: prepareChartItems(data),
    name: CHART_ITEMS[indicatorId].name,
    valueFormatter: CHART_ITEMS[indicatorId].valueFormatter,
  } ];
}
