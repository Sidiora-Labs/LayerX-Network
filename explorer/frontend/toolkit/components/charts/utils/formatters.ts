import type { TimeChartItem } from '../types';

export const getIncompleteDataLineSource = (data: Array<TimeChartItem>): Array<TimeChartItem> => {
  const result: Array<TimeChartItem> = [];

  for (let index = 0; index < data.length; index++) {
    const current = data[index];
    if (current.isApproximate) {
      const prev = data[index - 1];
      const next = data[index + 1];

      prev && !prev.isApproximate && result.push(prev);
      result.push(current);
      next && !next.isApproximate && result.push(next);
    }
  }

  return result;
};
