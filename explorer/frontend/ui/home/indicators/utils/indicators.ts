import type { TChainIndicator } from '../types';

import config from 'configs/app';

export const isIndicatorEnabled = ({ id }: TChainIndicator) => config.UI.homepage.charts.includes(id);

export const sortIndicators = (a: TChainIndicator, b: TChainIndicator) => {
  if (config.UI.homepage.charts.indexOf(a.id) > config.UI.homepage.charts.indexOf(b.id)) {
    return 1;
  }

  if (config.UI.homepage.charts.indexOf(a.id) < config.UI.homepage.charts.indexOf(b.id)) {
    return -1;
  }

  return 0;
};
