import { Box, Flex, Grid, GridItem } from '@chakra-ui/react';
import * as d3 from 'd3';
import React from 'react';

import type { AxesConfigFn } from 'toolkit/components/charts/types';
import type { UptimeHistoryFull, UptimeHistoryItem } from 'types/api/megaEth';

import { useSettingsContext } from 'lib/contexts/settings';
import { Heading } from 'toolkit/chakra/heading';
import { ChartWidget } from 'toolkit/components/charts/ChartWidget';
import { DAY, HOUR, SECOND } from 'toolkit/utils/consts';
import { useChartsConfig } from 'ui/shared/chart/config';
import TagGroupSelect from 'ui/shared/tagGroupSelect/TagGroupSelect';

const INTERVALS = [
  { id: '3h', title: '3h' } as const,
  { id: '24h', title: '24h' } as const,
  { id: '7d', title: '7d' } as const,
];

type IntervalId = (typeof INTERVALS)[number]['id'];

const TIME_FORMAT = '%e %b %Y, %H:%M:%S';

const AXES_CONFIG_BASE: (isLocalTime: boolean) => AxesConfigFn = (isLocalTime) => ({ isEnlarged, isMobile }) => ({
  y: {
    scale: { min: 0 },
  },
  x: {
    ticks: isEnlarged && !isMobile ? 8 : 5,
    tickFormatter: () => (d: d3.AxisDomain) => {
      return isLocalTime ? d3.timeFormat('%H:%M')(d as Date) : d3.utcFormat('%H:%M')(d as Date);
    },
  },
});

const AXES_CONFIG_LONG: AxesConfigFn = () => ({
  y: {
    scale: { min: 0 },
  },
});

const filterByInterval = (interval: IntervalId, now: number) => ({ date }: { date: Date }) => {
  switch (interval) {
    case '3h':
      return date.getTime() > now - 3 * HOUR;
    case '24h':
      return date.getTime() > now - 24 * HOUR;
    case '7d':
      return date.getTime() > now - 7 * DAY;
  }
};

// Algorithm provided by MegaETH team
const smoothData = (data: Array<UptimeHistoryItem>, windowSize: number): Array<UptimeHistoryItem> => {
  const result: Array<UptimeHistoryItem> = [];
  const halfWindow = Math.floor(windowSize / 2);

  for (let i = 0; i < data.length; i++) {
    const windowStart = Math.max(0, i - halfWindow);
    const windowEnd = Math.min(data.length, i + halfWindow + 1);
    const windowValues = data.slice(windowStart, windowEnd);

    const validValues = windowValues.filter((point) => point.value !== null);
    const average = validValues.length > 0 ?
      validValues.reduce((sum, point) => sum + (point.value || 0), 0) / validValues.length :
      null;

    const value = average || data[i].value || 0;

    result.push({ ...data[i], value });
  }
  return result;
};

interface Props {
  historyData: UptimeHistoryFull | null;
}

const UptimeCharts = ({ historyData }: Props) => {
  const [ interval, setInterval ] = React.useState<IntervalId>('3h');
  const chartsConfig = useChartsConfig();

  const { isLocalTime } = useSettingsContext() ?? { isLocalTime: true };

  const axesConfig = React.useMemo(() => {
    switch (interval) {
      case '3h':
      case '24h':
        return AXES_CONFIG_BASE(isLocalTime);
      case '7d':
        return AXES_CONFIG_LONG;
    }
  }, [ interval, isLocalTime ]);

  const tpsCharts = React.useMemo(() => {
    if (!historyData) {
      return [];
    }

    const data = (() => {
      switch (interval) {
        case '3h':
          return historyData.historical_tps_3h;
        case '7d':
          return historyData.historical_tps_7d;
        case '24h':
          return historyData.historical_tps_24h;
      }
    })();
    const now = Date.now();

    const smoothedData = smoothData(data, 7);
    const items = smoothedData
      .map(({ value, timestamp }) => {
        const date = new Date(timestamp * SECOND);
        return {
          date,
          value: Number(value.toFixed(0)),
          dateLabel: isLocalTime ? d3.timeFormat(TIME_FORMAT)(date) : d3.utcFormat(TIME_FORMAT)(date),
        };
      })
      .filter(filterByInterval(interval, now));

    return [
      {
        id: 'tps',
        name: 'Value',
        items,
        charts: chartsConfig,
      },
    ];
  }, [ chartsConfig, historyData, interval, isLocalTime ]);

  const gasCharts = React.useMemo(() => {
    if (!historyData) {
      return [];
    }

    const data = (() => {
      switch (interval) {
        case '3h':
          return historyData.historical_gas_per_second_3h;
        case '7d':
          return historyData.historical_gas_per_second_7d;
        case '24h':
          return historyData.historical_gas_per_second_24h;
      }
    })();

    const now = Date.now();

    const smoothedData = smoothData(data, 7);

    const items = smoothedData
      .map(({ value, timestamp }) => {
        const date = new Date(timestamp * SECOND);
        return {
          date,
          value: Number((value / 1_000_000).toFixed(2)),
          dateLabel: isLocalTime ? d3.timeFormat(TIME_FORMAT)(date) : d3.utcFormat(TIME_FORMAT)(date),
        };
      })
      .filter(filterByInterval(interval, now));

    return [
      {
        id: 'gas',
        name: 'Value',
        items,
        charts: chartsConfig,
      },
    ];
  }, [ chartsConfig, historyData, interval, isLocalTime ]);

  const blockIntervalCharts = React.useMemo(() => {
    if (!historyData) {
      return [];
    }

    const data = (() => {
      switch (interval) {
        case '3h':
          return historyData.historical_mini_block_interval_3h;
        case '7d':
          return historyData.historical_mini_block_interval_7d;
        case '24h':
          return historyData.historical_mini_block_interval_24h;
      }
    })();

    const now = Date.now();

    const smoothedData = smoothData(data, 7);

    const items = smoothedData
      .map(({ value, timestamp }) => {
        const date = new Date(timestamp * SECOND);
        return {
          date,
          value: Number(value.toFixed(1)),
          dateLabel: isLocalTime ? d3.timeFormat(TIME_FORMAT)(date) : d3.utcFormat(TIME_FORMAT)(date),
        };
      })
      .filter(filterByInterval(interval, now));

    return [
      {
        id: 'blockInterval',
        name: 'Value',
        items,
        charts: chartsConfig,
      },
    ];
  }, [ chartsConfig, historyData, interval, isLocalTime ]);

  const handleIntervalChange = React.useCallback((newInterval: IntervalId) => {
    setInterval(newInterval);
  }, [ setInterval ]);

  return (
    <Box>
      <Flex
        alignItems="center"
        justifyContent="space-between"
        mb={ 6 }
      >
        <Heading level="2">Real-time statistics</Heading>
        <TagGroupSelect<IntervalId> items={ INTERVALS } onChange={ handleIntervalChange } value={ interval } tagSize="lg"/>
      </Flex>
      <Grid
        templateColumns={{ base: '1fr', lg: 'repeat(2, 1fr)' }}
        gap={ 4 }
      >
        <GridItem colSpan={{ base: 1, lg: 2 }} minH={{ base: '220px', lg: '320px' }}>
          <ChartWidget
            title="TPS"
            charts={ tpsCharts }
            isLoading={ false }
            isError={ false }
            axesConfig={ axesConfig }
          />
        </GridItem>
        <GridItem minH={{ base: '220px', lg: '320px' }}>
          <ChartWidget
            title="MGas/s"
            charts={ gasCharts }
            isLoading={ false }
            isError={ false }
            axesConfig={ axesConfig }
          />
        </GridItem>
        <GridItem minH={{ base: '220px', lg: '320px' }}>
          <ChartWidget
            title="Block time (ms)"
            charts={ blockIntervalCharts }
            isLoading={ false }
            isError={ false }
            axesConfig={ axesConfig }
          />
        </GridItem>
      </Grid>
    </Box>
  );
};

export default React.memo(UptimeCharts);
