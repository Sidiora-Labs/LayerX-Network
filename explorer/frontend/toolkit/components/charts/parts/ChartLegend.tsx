import type { BoxProps } from '@chakra-ui/react';
import { Box, Circle, Text } from '@chakra-ui/react';
import React from 'react';

import type { TimeChartData } from '../types';

export interface ChartLegendProps extends BoxProps {
  data: TimeChartData;
  selectedIndexes?: Array<number>;
  onItemClick?: (index: number) => void;
}

export const ChartLegend = React.memo(({ data, selectedIndexes, onItemClick, ...props }: ChartLegendProps) => {
  const handleItemClick = React.useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      const itemIndex = (event.currentTarget as HTMLDivElement).getAttribute(
        'data-index',
      );
      onItemClick?.(Number(itemIndex));
    },
    [ onItemClick ],
  );

  return (
    <Box display="flex" columnGap={ 3 } { ...props }>
      { data.map((item, index) => {
        const isSelected = selectedIndexes?.includes(index);
        const lineColor = (() => {
          const lineChart = item.charts.find((chart) => chart.type === 'line');
          const areaChart = item.charts.find((chart) => chart.type === 'area');
          return (
            lineChart?.color || areaChart?.gradient.startColor || 'transparent'
          );
        })();

        return (
          <Box
            key={ item.name }
            data-index={ index }
            display="flex"
            alignItems="center"
            columnGap={ 1 }
            p="2px"
            onClick={ handleItemClick }
            cursor="pointer"
          >
            <Circle
              size={ 2 }
              bgColor={ isSelected ? lineColor : 'transparent' }
              borderWidth={ 2 }
              borderColor={ lineColor }
            />
            <Text fontSize="xs" color="text.secondary">
              { item.name }
            </Text>
          </Box>
        );
      }) }
    </Box>
  );
});
