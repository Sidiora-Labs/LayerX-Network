import { chakra } from '@chakra-ui/react';
import React, { useEffect } from 'react';

import { Resolution } from '@blockscout/stats-types';
import type { StatsIntervalIds } from 'types/client/stats';

import { route, type Route } from 'nextjs-routes';

import config from 'configs/app';
import { ChartWidget } from 'toolkit/components/charts/ChartWidget';
import { useChartsConfig } from 'ui/shared/chart/config';
import useChartQuery from 'ui/shared/chart/useChartQuery';

type Props = {
  id: string;
  title: string;
  description: string;
  interval: StatsIntervalIds;
  onLoadingError: () => void;
  isPlaceholderData: boolean;
  className?: string;
  href?: Route;
};

const ChartWidgetContainer = ({
  id,
  title,
  description,
  interval,
  onLoadingError,
  isPlaceholderData,
  className,
  href,
}: Props) => {
  const { items, lineQuery } = useChartQuery(id, Resolution.DAY, interval, !isPlaceholderData);
  const chartsConfig = useChartsConfig();

  useEffect(() => {
    if (lineQuery.isError) {
      onLoadingError();
    }
  }, [ lineQuery.isError, onLoadingError ]);

  const charts = React.useMemo(() => {
    if (!lineQuery.data?.info || !items) {
      return [];
    }

    return [
      {
        id: lineQuery.data?.info?.id,
        name: 'Value',
        items,
        charts: chartsConfig,
        units: lineQuery.data.info.units,
      },
    ];
  }, [ lineQuery.data?.info, items, chartsConfig ]);

  return (
    <ChartWidget
      isError={ lineQuery.isError }
      charts={ charts }
      title={ title }
      description={ description }
      isLoading={ lineQuery.isPlaceholderData }
      minH="230px"
      className={ className }
      href={ href ? route(href) : undefined }
      chartUrl={ href ? `${ config.app.baseUrl }${ route(href) }` : undefined }
    />
  );
};

export default chakra(ChartWidgetContainer);
