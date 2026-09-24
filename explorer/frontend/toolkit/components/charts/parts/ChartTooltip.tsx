import * as d3 from 'd3';
import React from 'react';

import { Resolution } from '../types';
import type { TimeChartData } from '../types';

import ChartTooltipBackdrop, { useRenderBackdrop } from './tooltip/ChartTooltipBackdrop';
import ChartTooltipContent, { useRenderContent } from './tooltip/ChartTooltipContent';
import ChartTooltipLine, { useRenderLine } from './tooltip/ChartTooltipLine';
import ChartTooltipPoint, { useRenderPoints } from './tooltip/ChartTooltipPoint';
import ChartTooltipRow, { useRenderRows } from './tooltip/ChartTooltipRow';
import ChartTooltipTitle, { useRenderTitle } from './tooltip/ChartTooltipTitle';
import { trackPointer } from './tooltip/pointerTracker';
import type { Pointer } from './tooltip/pointerTracker';

export interface ChartTooltipProps {
  width?: number;
  tooltipWidth?: number;
  height?: number;
  data: TimeChartData;
  xScale: d3.ScaleTime<number, number>;
  yScale: d3.ScaleLinear<number, number>;
  anchorEl: SVGRectElement | null;
  noAnimation?: boolean;
  resolution?: Resolution;
}

export const ChartTooltip = React.memo(({
  xScale,
  yScale,
  width,
  tooltipWidth = 200,
  height,
  data,
  anchorEl,
  noAnimation,
  resolution,
  ...props
}: ChartTooltipProps) => {
  const ref = React.useRef<SVGGElement>(null);
  const trackerId = React.useRef<number>(undefined);
  const isVisible = React.useRef(false);

  const transitionDuration = !noAnimation ? 100 : null;

  const renderLine = useRenderLine(ref, height);
  const renderContent = useRenderContent(ref, { chart: { width, height }, transitionDuration });
  const renderPoints = useRenderPoints(ref, { data, xScale, yScale });
  const renderTitle = useRenderTitle(ref);
  const renderRows = useRenderRows(ref, { data, xScale, minWidth: tooltipWidth });
  const renderBackdrop = useRenderBackdrop(ref, { seriesNum: data.length, transitionDuration });

  const draw = React.useCallback((pointer: Pointer) => {
    if (pointer.point) {
      const { x, y, currentPoints } = renderPoints(pointer.point[0]);
      const isIncompleteData = currentPoints.some(({ item }) => item.isApproximate);
      renderLine(x);
      renderContent(x, y);
      renderTitle(isIncompleteData);
      const { width } = renderRows(x, currentPoints);
      renderBackdrop(width, isIncompleteData);
    }
  }, [ renderPoints, renderLine, renderContent, renderTitle, renderRows, renderBackdrop ]);

  const showContent = React.useCallback(() => {
    if (!isVisible.current) {
      if (transitionDuration) {
        d3.select(ref.current)
          .transition()
          .delay(transitionDuration)
          .attr('opacity', 1);
      } else {
        d3.select(ref.current)
          .attr('opacity', 1);
      }
      isVisible.current = true;
    }
  }, [ transitionDuration ]);

  const hideContent = React.useCallback(() => {
    if (transitionDuration) {
      d3.select(ref.current)
        .transition()
        .delay(transitionDuration)
        .attr('opacity', 0);
    } else {
      d3.select(ref.current)
        .attr('opacity', 0);
    }
    isVisible.current = false;
  }, [ transitionDuration ]);

  const createPointerTracker = React.useCallback((event: PointerEvent, isSubsequentCall?: boolean) => {
    let isPressed = event.pointerType === 'mouse' && event.type === 'pointerdown' && !isSubsequentCall;

    if (isPressed) {
      hideContent();
    }

    return trackPointer(event, {
      move: (pointer) => {
        if (!pointer.point || isPressed) {
          return;
        }
        draw(pointer);
        showContent();
      },
      out: () => {
        hideContent();
        trackerId.current = undefined;
      },
      end: () => {
        hideContent();
        trackerId.current = undefined;
        isPressed = false;
      },
    });
  }, [ draw, hideContent, showContent ]);

  React.useEffect(() => {
    const anchorD3 = d3.select(anchorEl);
    let isMultiTouch = false; // disabling creation of new tracker in multi touch mode

    anchorD3
      .on('touchmove.tooltip', (event: TouchEvent) => event.preventDefault()) // prevent scrolling
      .on(`touchstart.tooltip`, (event: TouchEvent) => {
        isMultiTouch = event.touches.length > 1;
      }, { passive: true })
      .on(`touchend.tooltip`, (event: TouchEvent) => {
        if (isMultiTouch && event.touches.length === 0) {
          isMultiTouch = false;
        }
      }, { passive: true })
      .on('pointerenter.tooltip pointerdown.tooltip', (event: PointerEvent) => {
        if (!isMultiTouch) {
          trackerId.current = createPointerTracker(event);
        }
      }, { passive: true })
      .on('pointermove.tooltip', (event: PointerEvent) => {
        if (event.pointerType === 'mouse' && !isMultiTouch && trackerId.current === undefined) {
          trackerId.current = createPointerTracker(event);
        }
      }, { passive: true });

    return () => {
      anchorD3.on('touchmove.tooltip pointerenter.tooltip pointerdown.tooltip', null);
      trackerId.current && anchorD3.on(
        [ 'pointerup', 'pointercancel', 'lostpointercapture', 'pointermove', 'pointerout' ].map((event) => `${ event }.${ trackerId.current }`).join(' '),
        null,
      );
    };
  }, [ anchorEl, createPointerTracker, draw, hideContent, showContent ]);

  const lastItemDateString = data[0].items[data[0].items.length - 1]?.date?.toISOString();

  React.useEffect(() => {
    if (trackerId.current) {
      trackerId.current = undefined;
      hideContent();
    }
  }, [ hideContent, lastItemDateString ]);

  return (
    <g
      ref={ ref }
      opacity={ 0 }
      style={{
        fontWeight: '500',
        fontSize: '12px',
      }}
      { ...props }
    >
      <ChartTooltipLine/>
      { data.map(({ name }) => <ChartTooltipPoint key={ name }/>) }
      <ChartTooltipContent>
        <ChartTooltipBackdrop/>
        <ChartTooltipTitle resolution={ resolution }/>
        <ChartTooltipRow label={ getDateLabel(resolution) } lineNum={ 1 }/>
        { data.map(({ name }, index) => <ChartTooltipRow key={ name } label={ name } lineNum={ index + 1 }/>) }
      </ChartTooltipContent>
    </g>
  );
});

function getDateLabel(resolution?: Resolution): string {
  switch (resolution) {
    case Resolution.WEEK:
      return 'Dates';
    case Resolution.MONTH:
      return 'Month';
    case Resolution.YEAR:
      return 'Year';
    default:
      return 'Date';
  }
}
