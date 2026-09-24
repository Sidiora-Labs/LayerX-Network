import type { TimeChartItem } from 'toolkit/components/charts/types';

export function formatDate(date: Date) {
  return date.toISOString().substring(0, 10);
}

export const sortByDateDesc = (a: Pick<TimeChartItem, 'date'>, b: Pick<TimeChartItem, 'date'>) => {
  return a.date.getTime() - b.date.getTime();
};
