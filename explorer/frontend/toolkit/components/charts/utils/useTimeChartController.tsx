import React from 'react';

import type { AxesConfig, ChartMargin, TimeChartData } from '../types';

import { useClientRect } from '../../../hooks/useClientRect';
import { calculateInnerSize } from './calculateInnerSize';
import { getAxesParams } from './timeChartAxis';

interface Props {
  data: TimeChartData;
  margin?: ChartMargin;
  axesConfig?: AxesConfig;
}

export function useTimeChartController({ data, margin, axesConfig }: Props) {

  const [ rect, ref ] = useClientRect<SVGSVGElement>();

  // we need to recalculate the axis scale whenever the rect width changes
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const axesParams = React.useMemo(() => getAxesParams(data, axesConfig), [ data, axesConfig, rect?.width ]);

  const chartMargin = React.useMemo(() => {
    const PIXELS_PER_DIGIT = 8;
    const leftShift = axesConfig?.y?.noLabel ? 0 : PIXELS_PER_DIGIT * axesParams.y.labelFormatParams.maxLabelLength;

    return {
      ...margin,
      left: (margin?.left ?? 0) + leftShift,
    };
  }, [ axesParams.y.labelFormatParams.maxLabelLength, margin, axesConfig?.y?.noLabel ]);

  const { innerWidth, innerHeight } = calculateInnerSize(rect, chartMargin);

  const xScale = React.useMemo(() => {
    return axesParams.x.scale.range([ 0, innerWidth ]);
  }, [ axesParams.x.scale, innerWidth ]);

  const yScale = React.useMemo(() => {
    return axesParams.y.scale.range([ innerHeight, 0 ]);
  }, [ axesParams.y.scale, innerHeight ]);

  return React.useMemo(() => {
    return {
      rect,
      ref,
      chartMargin,
      innerWidth,
      innerHeight,
      axes: {
        x: {
          tickFormatter: axesParams.x.tickFormatter,
          scale: xScale,
        },
        y: {
          tickFormatter: axesParams.y.tickFormatter,
          scale: yScale,
        },
      },
    };
  }, [ axesParams.x.tickFormatter, axesParams.y.tickFormatter, chartMargin, innerHeight, innerWidth, rect, ref, xScale, yScale ]);
}
