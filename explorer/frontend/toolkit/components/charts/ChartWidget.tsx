import type { FlexProps } from '@chakra-ui/react';
import { Flex, Icon } from '@chakra-ui/react';
import { range } from 'es-toolkit';
import React, { useRef } from 'react';

import type { AxesConfigFn, TimeChartData } from './types';

import RepeatIcon from 'icons/repeat.svg';

import { IconButton } from '../../chakra/icon-button';
import { Link } from '../../chakra/link';
import { Skeleton } from '../../chakra/skeleton';
import { Tooltip } from '../../chakra/tooltip';
import { ChartWidgetContent } from './ChartWidgetContent';
import { ChartLegend } from './parts/ChartLegend';
import type { ChartMenuItemId } from './parts/ChartMenu';
import ChartMenu from './parts/ChartMenu';
import { useChartZoom } from './utils/useChartZoom';

export interface ChartWidgetProps extends FlexProps {
  charts: TimeChartData;
  title: string;
  description?: string;
  isLoading: boolean;
  isError: boolean;
  emptyText?: string;
  noAnimation?: boolean;
  href?: string;
  chartUrl?: string;
  axesConfig?: AxesConfigFn;
  menuItemIds?: Array<ChartMenuItemId>;
  noWatermark?: boolean;
};

export const ChartWidget = React.memo(({
  charts,
  title,
  description,
  isLoading,
  isError,
  emptyText,
  noAnimation,
  href,
  chartUrl,
  axesConfig,
  menuItemIds,
  noWatermark,
  ...rest
}: ChartWidgetProps) => {
  const ref = useRef<HTMLDivElement>(null);

  const { zoomRange, handleZoom, handleZoomReset } = useChartZoom();

  const [ selectedCharts, setSelectedCharts ] = React.useState<Array<number>>(
    range(charts.length),
  );

  React.useEffect(() => {
    if (charts.length > 0) {
      setSelectedCharts(range(charts.length));
    }
  }, [ charts.length ]);

  const handleLegendItemClick = React.useCallback((index: number) => {
    setSelectedCharts((prev) => {
      if (prev.includes(index)) {
        return prev.filter((item) => item !== index);
      }
      return [ ...prev, index ];
    });
  }, []);

  const displayedCharts = React.useMemo(() => {
    return charts.filter((_, index) => selectedCharts.includes(index));
  }, [ charts, selectedCharts ]);

  const hasNonEmptyCharts = charts.some(({ items }) => items && items.length > 2);
  const hasMenu = (() => {
    const hasIds = !(menuItemIds && menuItemIds.length === 0);
    if (!hasIds) {
      return false;
    }
    if (isError) {
      return false;
    }
    if (!hasNonEmptyCharts) {
      return false;
    }
    return true;
  })();

  const content = (
    <ChartWidgetContent
      charts={ displayedCharts }
      isError={ isError }
      isLoading={ isLoading }
      empty={ !hasNonEmptyCharts }
      emptyText={ emptyText }
      handleZoom={ handleZoom }
      zoomRange={ zoomRange }
      noAnimation={ noAnimation }
      axesConfig={ axesConfig }
      noWatermark={ noWatermark }
    />
  );

  const chartHeader = (
    <Flex
      flexGrow={ 1 }
      flexDir="column"
      alignItems="flex-start"
      cursor={ href ? 'pointer' : 'default' }
      _hover={ href ? { color: 'link.primary.hovered' } : {} }
    >
      <Skeleton
        loading={ isLoading }
        fontWeight={ 600 }
        textStyle="md"
      >
        <span>{ title }</span>
      </Skeleton>

      { description && (
        <Skeleton
          loading={ isLoading }
          color="text.secondary"
          textStyle="xs"
          mt={ 1 }
        >
          <span>{ description }</span>
        </Skeleton>
      ) }
    </Flex>
  );

  return (
    <Flex
      height="100%"
      ref={ ref }
      flexDir="column"
      padding={{ base: 3, lg: 4 }}
      borderRadius="lg"
      borderWidth="1px"
      borderColor={{ _light: 'gray.200', _dark: 'gray.600' }}
      { ...rest }
    >
      <Flex columnGap={ 6 } mb={ 2 } alignItems="flex-start">
        { href ? (
          <Link href={ href }>
            { chartHeader }
          </Link>
        ) : chartHeader }
        <Flex ml="auto" columnGap={ 2 }>
          <Tooltip content="Reset zoom">
            <IconButton
              hidden={ !zoomRange }
              aria-label="Reset zoom"
              size="md"
              variant="icon_background"
              onClick={ handleZoomReset }
            >
              <Icon><RepeatIcon/></Icon>
            </IconButton>
          </Tooltip>

          { hasMenu && (
            <ChartMenu
              charts={ charts }
              itemIds={ menuItemIds }
              title={ title }
              description={ description }
              chartUrl={ chartUrl }
              isLoading={ isLoading }
              chartRef={ ref }
              handleZoom={ handleZoom }
              handleZoomReset={ handleZoomReset }
              zoomRange={ zoomRange }
            />
          ) }
        </Flex>
      </Flex>

      { content }

      { charts.length > 1 && (
        <ChartLegend
          data={ charts }
          selectedIndexes={ selectedCharts }
          onItemClick={ handleLegendItemClick }
        />
      ) }
    </Flex>
  );
});
