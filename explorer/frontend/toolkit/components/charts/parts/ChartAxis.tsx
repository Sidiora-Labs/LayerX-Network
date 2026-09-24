import { useToken } from '@chakra-ui/react';
import * as d3 from 'd3';
import React from 'react';

import { useColorModeValue } from '../../../chakra/color-mode';

export interface ChartAxisProps extends Omit<React.SVGProps<SVGGElement>, 'scale'> {
  type: 'left' | 'bottom';
  scale: d3.ScaleTime<number, number> | d3.ScaleLinear<number, number>;
  noAnimation?: boolean;
  ticks: number;
  tickFormatGenerator?: (axis: d3.Axis<d3.NumberValue>) => (domainValue: d3.AxisDomain, index: number) => string;
  anchorEl?: SVGRectElement | null;
}

export const ChartAxis = React.memo(({ type, scale, ticks, tickFormatGenerator, noAnimation, anchorEl, ...props }: ChartAxisProps) => {
  const ref = React.useRef<SVGGElement>(null);

  const textColor = useToken('colors', useColorModeValue('blackAlpha.600', 'whiteAlpha.500'));

  React.useEffect(() => {
    if (!ref.current) {
      return;
    }

    const axisGenerator = type === 'left' ? d3.axisLeft : d3.axisBottom;
    const axis = axisGenerator(scale).ticks(ticks);

    if (tickFormatGenerator) {
      axis.tickFormat(tickFormatGenerator(axis));
    }

    const axisGroup = d3.select(ref.current);

    if (noAnimation) {
      axisGroup.call(axis);
    } else {
      axisGroup.transition().duration(750).ease(d3.easeLinear).call(axis);
    }
    axisGroup.select('.domain').remove();
    axisGroup.selectAll('line').remove();
    axisGroup.selectAll('text')
      .attr('opacity', 1)
      .attr('color', textColor)
      .style('font-size', '12px');
  }, [ scale, ticks, tickFormatGenerator, noAnimation, type, textColor ]);

  React.useEffect(() => {
    if (!anchorEl) {
      return;
    }

    const anchorD3 = d3.select(anchorEl);

    anchorD3
      .on('mouseout.axisX', () => {
        d3.select(ref.current)
          .selectAll('text')
          .style('font-weight', 'normal');
      })
      .on('mousemove.axisX', (event) => {
        const [ x ] = d3.pointer(event, anchorEl);
        const xDate = scale.invert(x);
        const textElements = d3.select(ref.current).selectAll('text');
        const data = textElements.data();
        const index = d3.bisector((d) => d).left(data, xDate);
        textElements
          .style('font-weight', (_, i) => i === index - 1 ? 'bold' : 'normal');
      });

    return () => {
      anchorD3.on('mouseout.axisX mousemove.axisX', null);
    };
  }, [ anchorEl, scale ]);

  return <g ref={ ref } { ...props }/>;
});
