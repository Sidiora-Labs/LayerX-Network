import { useToken } from '@chakra-ui/react';
import * as d3 from 'd3';
import React from 'react';

import { calculateContainerHeight } from './utils';

const ChartTooltipBackdrop = () => {
  const bgColor = useToken('colors', 'blackAlpha.900');

  return (
    <rect
      className="ChartTooltip__backdrop"
      rx={ 12 }
      ry={ 12 }
      fill={ bgColor[0] }
    />
  );
};

export default React.memo(ChartTooltipBackdrop);

interface UseRenderBackdropParams {
  seriesNum: number;
  transitionDuration: number | null;
}

export function useRenderBackdrop(ref: React.RefObject<SVGGElement | null>, { seriesNum, transitionDuration }: UseRenderBackdropParams) {
  return React.useCallback((width: number, isIncompleteData: boolean) => {
    const height = calculateContainerHeight(seriesNum, isIncompleteData);

    if (transitionDuration) {
      d3.select(ref.current)
        .select('.ChartTooltip__backdrop')
        .transition()
        .duration(transitionDuration)
        .ease(d3.easeLinear)
        .attr('width', width)
        .attr('height', height);
    } else {
      d3.select(ref.current)
        .select('.ChartTooltip__backdrop')
        .attr('width', width)
        .attr('height', height);
    }
  }, [ ref, seriesNum, transitionDuration ]);
}
