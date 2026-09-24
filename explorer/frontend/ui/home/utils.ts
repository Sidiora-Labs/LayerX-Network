import type { HomeStatsWidgetId } from 'types/homepage';

import config from 'configs/app';
import type { Props as StatsWidgetProps } from 'ui/shared/stats/StatsWidget';

export interface HomeStatsItem extends StatsWidgetProps {
  id: HomeStatsWidgetId;
}

export const isHomeStatsItemEnabled = (item: HomeStatsItem) => config.UI.homepage.stats.includes(item.id);

export const sortHomeStatsItems = (a: HomeStatsItem, b: HomeStatsItem) => {
  const indexA = config.UI.homepage.stats.indexOf(a.id);
  const indexB = config.UI.homepage.stats.indexOf(b.id);
  if (indexA > indexB) {
    return 1;
  }
  if (indexA < indexB) {
    return -1;
  }
  return 0;
};
