import React from 'react';

import type { ChartWidgetProps } from 'toolkit/components/charts/ChartWidget';
import { ChartWidget } from 'toolkit/components/charts/ChartWidget';

import { useChartsConfig } from './config';

const ChartWidgetPwStory = ({ charts, ...props }: ChartWidgetProps) => {
  const chartsConfig = useChartsConfig();

  const modifiedCharts = React.useMemo(() => {
    return charts.map((chart) => {
      if (chart.charts.length === 0) {
        return {
          ...chart,
          charts: chartsConfig,
        };
      }
      return chart;
    });
  }, [ charts, chartsConfig ]);

  return <ChartWidget { ...props } charts={ modifiedCharts }/>;
};

export default React.memo(ChartWidgetPwStory);
