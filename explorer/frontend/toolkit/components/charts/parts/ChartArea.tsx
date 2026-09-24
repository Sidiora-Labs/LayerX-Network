import * as d3 from 'd3';
import React from 'react';

import type { TimeChartItem } from '../types';

export interface ChartAreaProps extends React.SVGProps<SVGPathElement> {
  id: string;
  xScale: d3.ScaleTime<number, number> | d3.ScaleLinear<number, number>;
  yScale: d3.ScaleTime<number, number> | d3.ScaleLinear<number, number>;
  gradient: {
    startColor: string;
    stopColor: string;
  };
  data: Array<TimeChartItem>;
  noAnimation?: boolean;
}

export const ChartArea = React.memo(({ id, xScale, yScale, gradient, data, noAnimation, ...props }: ChartAreaProps) => {
  const ref = React.useRef(null);

  const gradientId = `gradient-chart-area-${ id }`;

  React.useEffect(() => {
    if (noAnimation) {
      d3.select(ref.current).attr('opacity', 1);
      return;
    }
    d3.select(ref.current).transition()
      .duration(750)
      .ease(d3.easeBackIn)
      .attr('opacity', 1);
  }, [ noAnimation ]);

  const d = React.useMemo(() => {
    const area = d3.area<TimeChartItem>()
      .defined(({ isApproximate }) => !isApproximate)
      .x(({ date }) => xScale(date))
      .y1(({ value }) => yScale(value))
      .y0(() => yScale(yScale.domain()[0]))
      .curve(d3.curveMonotoneX);
    return area(data) || undefined;
  }, [ xScale, yScale, data ]);

  return (
    <>
      <path
        ref={ ref }
        d={ d }
        fill={ `url(#${ gradientId })` }
        opacity={ 0 }
        data-name={ id || 'gradient-chart-area' }
        { ...props }
      />
      <defs>
        <linearGradient id={ gradientId } x1="0%" x2="0%" y1="0%" y2="100%">
          <stop offset="0%" stopColor={ gradient.startColor }/>
          <stop offset="100%" stopColor={ gradient.stopColor }/>
        </linearGradient>
      </defs>
    </>
  );
});
