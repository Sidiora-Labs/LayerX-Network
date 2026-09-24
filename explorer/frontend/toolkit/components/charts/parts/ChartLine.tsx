import * as d3 from 'd3';
import React from 'react';

import type { TimeChartItem } from '../types';

import type { AnimationType } from '../utils/animations';
import { ANIMATIONS } from '../utils/animations';
import { getIncompleteDataLineSource } from '../utils/formatters';

export interface ChartLineProps extends React.SVGProps<SVGPathElement> {
  xScale: d3.ScaleTime<number, number> | d3.ScaleLinear<number, number>;
  yScale: d3.ScaleTime<number, number> | d3.ScaleLinear<number, number>;
  data: Array<TimeChartItem>;
  animation: AnimationType;
}

export const ChartLine = React.memo(({ xScale, yScale, data, animation, strokeDasharray, ...props }: ChartLineProps) => {
  const dataPathRef = React.useRef<SVGPathElement>(null);
  const incompleteDataPathRef = React.useRef<SVGPathElement>(null);

  React.useEffect(() => {
    const animationFn = ANIMATIONS[animation];
    const timeoutId = window.setTimeout(() => {
      dataPathRef.current && animationFn(dataPathRef.current);
      incompleteDataPathRef.current && animationFn(incompleteDataPathRef.current);
    }, 100);

    return () => {
      window.clearTimeout(timeoutId);
    };
  }, [ animation ]);

  // Recalculate line length if scale has changed
  React.useEffect(() => {
    if (animation === 'left') {
      const totalLength = dataPathRef.current?.getTotalLength();
      d3.select(dataPathRef.current).attr(
        'stroke-dasharray',
        `${ totalLength },${ totalLength }`,
      );
    }
  }, [ xScale, yScale, animation ]);

  const line = d3.line<TimeChartItem>()
    .x((d) => xScale(d.date))
    .y((d) => yScale(d.value))
    .curve(d3.curveMonotoneX);

  return (
    <>
      <path
        ref={ incompleteDataPathRef }
        d={ line(getIncompleteDataLineSource(data)) || undefined }
        strokeWidth={ 1 }
        strokeLinecap="round"
        fill="none"
        strokeDasharray="6 6"
        opacity={ 0 }
        { ...props }
      />
      <path
        ref={ dataPathRef }
        d={ line(data.filter(({ isApproximate }) => !isApproximate)) || undefined }
        strokeWidth={ 1 }
        strokeLinecap="round"
        strokeDasharray={ strokeDasharray }
        fill="none"
        opacity={ 0 }
        { ...props }
      />
    </>
  );
});
