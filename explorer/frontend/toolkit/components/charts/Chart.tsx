import { defaultsDeep } from 'es-toolkit/compat';
import React from 'react';

import type { AxesConfigFn, ChartMargin, TimeChartData } from './types';
import { Resolution } from './types';

import useIsMobile from 'lib/hooks/useIsMobile';

import { ChartArea } from './parts/ChartArea';
import { ChartAxis } from './parts/ChartAxis';
import { ChartGridLine } from './parts/ChartGridLine';
import { ChartLine } from './parts/ChartLine';
import { ChartOverlay } from './parts/ChartOverlay';
import { ChartSelectionX } from './parts/ChartSelectionX';
import { ChartTooltip } from './parts/ChartTooltip';
import { getDateLabel } from './utils/getDateLabel';
import { useTimeChartController } from './utils/useTimeChartController';

export interface ChartProps {
  charts: TimeChartData;
  zoomRange?: [ Date, Date ];
  onZoom: (range: [ Date, Date ]) => void;
  margin?: ChartMargin;
  noAnimation?: boolean;
  resolution?: Resolution;
  axesConfig?: AxesConfigFn;
  isEnlarged?: boolean;
}

const DEFAULT_CHART_MARGIN = { bottom: 20, left: 10, right: 20, top: 10 };

export const Chart = React.memo(({
  isEnlarged,
  charts,
  onZoom,
  margin: marginProps,
  noAnimation,
  resolution,
  zoomRange,
  axesConfig: axesConfigProps,
}: ChartProps) => {
  const isMobile = useIsMobile();

  const overlayRef = React.useRef<SVGRectElement>(null);

  const range = React.useMemo(
    () =>
      zoomRange || [
        charts[0].items[0].date,
        charts[0].items[charts[0].items.length - 1].date,
      ],
    [ zoomRange, charts ],
  );

  const displayedCharts = React.useMemo(() => {
    return charts.map((chart) => ({
      ...chart,
      items: chart.items
        .filter((item) => item.date >= range[0] && item.date <= range[1])
        .map((item) => (item.dateLabel ? item : {
          ...item,
          dateLabel: getDateLabel(item.date, item.date_to, resolution),
        })),
    }));
  }, [ charts, range, resolution ]);

  const margin: ChartMargin = React.useMemo(() => ({ ...DEFAULT_CHART_MARGIN, ...marginProps }), [ marginProps ]);

  const axesConfig = React.useMemo(() => {
    return defaultsDeep(axesConfigProps?.({ isEnlarged, isMobile }), {
      x: {
        ticks: isEnlarged && !isMobile ? 8 : 4,
      },
      y: {
        ticks: isEnlarged ? 6 : 3,
        nice: true,
      },
    });
  }, [ axesConfigProps, isEnlarged, isMobile ]);

  const {
    ref,
    rect,
    innerWidth,
    innerHeight,
    chartMargin,
    axes,
  } = useTimeChartController({
    data: displayedCharts,
    margin,
    axesConfig,
  });

  return (
    <svg width="100%" height="100%" ref={ ref } cursor="pointer" opacity={ rect ? 1 : 0 }>

      <g transform={ `translate(${ chartMargin?.left || 0 },${ chartMargin?.top || 0 })` }>
        <ChartGridLine
          type="horizontal"
          scale={ axes.y.scale }
          ticks={ axesConfig.y.ticks }
          size={ innerWidth }
          noAnimation
        />

        { displayedCharts.map((chart) => {
          const id = `${ chart.id }-${ isEnlarged ? 'fullscreen' : 'small' }`;
          return (
            <React.Fragment key={ id }>
              { chart.charts.map((chartConfig) => {

                if (chartConfig.type === 'area') {
                  return (
                    <ChartArea
                      key={ chartConfig.type }
                      id={ id }
                      data={ chart.items }
                      gradient={ chartConfig.gradient }
                      xScale={ axes.x.scale }
                      yScale={ axes.y.scale }
                      noAnimation={ noAnimation }
                    />
                  );
                }

                return (
                  <ChartLine
                    key={ chartConfig.type }
                    data={ chart.items }
                    xScale={ axes.x.scale }
                    yScale={ axes.y.scale }
                    stroke={ chartConfig.color }
                    strokeWidth={ chartConfig.strokeWidth || 2 }
                    strokeDasharray={ chartConfig.strokeDasharray }
                    animation="none"
                  />
                );
              }) }
            </React.Fragment>
          );
        }) }

        <ChartAxis
          type="left"
          scale={ axes.y.scale }
          ticks={ axesConfig.y.ticks }
          tickFormatGenerator={ axes.y.tickFormatter }
          noAnimation
        />

        <ChartAxis
          type="bottom"
          scale={ axes.x.scale }
          transform={ `translate(0, ${ innerHeight })` }
          ticks={ axesConfig.x.ticks }
          anchorEl={ overlayRef.current }
          tickFormatGenerator={ axes.x.tickFormatter }
          noAnimation
        />

        <ChartOverlay ref={ overlayRef } width={ innerWidth } height={ innerHeight }>
          <ChartTooltip
            anchorEl={ overlayRef.current }
            width={ innerWidth }
            tooltipWidth={ (resolution === Resolution.WEEK) ? 280 : 200 }
            height={ innerHeight }
            xScale={ axes.x.scale }
            yScale={ axes.y.scale }
            data={ displayedCharts }
            noAnimation={ noAnimation }
            resolution={ resolution }
          />

          <ChartSelectionX
            anchorEl={ overlayRef.current }
            height={ innerHeight }
            scale={ axes.x.scale }
            data={ displayedCharts }
            onSelect={ onZoom }
          />
        </ChartOverlay>
      </g>
    </svg>
  );
});
