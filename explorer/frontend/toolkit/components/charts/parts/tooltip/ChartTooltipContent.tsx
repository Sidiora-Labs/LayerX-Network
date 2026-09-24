import * as d3 from 'd3';
import { clamp } from 'es-toolkit';
import React from 'react';

import { POINT_SIZE } from './utils';

interface Props {
  children: React.ReactNode;
}

const ChartTooltipContent = ({ children }: Props) => {
  return <g className="ChartTooltip__content">{ children }</g>;
};

export default React.memo(ChartTooltipContent);

interface UseRenderContentParams {
  chart: {
    width?: number;
    height?: number;
  };
  transitionDuration: number | null;
}

export function useRenderContent(ref: React.RefObject<SVGGElement | null>, { chart, transitionDuration }: UseRenderContentParams) {
  return React.useCallback((x: number, y: number) => {
    const tooltipContent = d3.select(ref.current).select('.ChartTooltip__content');

    const transformAttributeFn: d3.ValueFn<d3.BaseType, unknown, string> = (_, i, nodes) => {
      const node = nodes[i] as SVGGElement | null;
      const { width: nodeWidth, height: nodeHeight } = node?.getBoundingClientRect() || { width: 0, height: 0 };
      const [ translateX, translateY ] = calculatePosition({
        canvasWidth: chart.width || 0,
        canvasHeight: chart.height || 0,
        nodeWidth,
        nodeHeight,
        pointX: x,
        pointY: y,
        offset: POINT_SIZE,
      });
      return `translate(${ translateX }, ${ translateY })`;
    };

    if (transitionDuration) {
      tooltipContent
        .transition()
        .duration(transitionDuration)
        .ease(d3.easeLinear)
        .attr('transform', transformAttributeFn);
    } else {
      tooltipContent
        .attr('transform', transformAttributeFn);
    }

  }, [ chart.height, chart.width, ref, transitionDuration ]);
}

interface CalculatePositionParams {
  pointX: number;
  pointY: number;
  offset: number;
  nodeWidth: number;
  nodeHeight: number;
  canvasWidth: number;
  canvasHeight: number;
}

function calculatePosition({ pointX, pointY, canvasWidth, canvasHeight, nodeWidth, nodeHeight, offset }: CalculatePositionParams): [ number, number ] {
  // right
  if (pointX + offset + nodeWidth <= canvasWidth) {
    const x = pointX + offset;
    const y = clamp(pointY - nodeHeight / 2, 0, canvasHeight - nodeHeight);
    return [ x, y ];
  }

  // left
  if (nodeWidth + offset <= pointX) {
    const x = pointX - offset - nodeWidth;
    const y = clamp(pointY - nodeHeight / 2, 0, canvasHeight - nodeHeight);
    return [ x, y ];
  }

  // top
  if (nodeHeight + offset <= pointY) {
    const x = clamp(pointX - nodeWidth / 2, 0, canvasWidth - nodeWidth);
    const y = pointY - offset - nodeHeight;
    return [ x, y ];
  }

  // bottom
  if (pointY + offset + nodeHeight <= canvasHeight) {
    const x = clamp(pointX - nodeWidth / 2, 0, canvasWidth - nodeWidth);
    const y = pointY + offset;
    return [ x, y ];
  }

  const x = clamp(pointX / 2, 0, canvasWidth - nodeWidth);
  const y = clamp(pointY / 2, 0, canvasHeight - nodeHeight);

  return [ x, y ];
}
