import { useToken } from '@chakra-ui/react';
import * as d3 from 'd3';
import React from 'react';

const ChartTooltipLine = () => {
  const lineColor = useToken('colors', 'gray.400');
  return <line className="ChartTooltip__line" stroke={ lineColor[0] } strokeDasharray="3"/>;
};

export default React.memo(ChartTooltipLine);

export function useRenderLine(ref: React.RefObject<SVGGElement | null>, chartHeight: number | undefined) {
  return React.useCallback((x: number) => {
    d3.select(ref.current)
      .select('.ChartTooltip__line')
      .attr('x1', x)
      .attr('x2', x)
      .attr('y1', 0)
      .attr('y2', chartHeight || 0);
  }, [ ref, chartHeight ]);
}
