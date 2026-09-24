import { useToken } from '@chakra-ui/react';
import React from 'react';

import type { ChartConfig } from 'toolkit/components/charts/types';

import useIsMobile from 'lib/hooks/useIsMobile';
import { useColorModeValue } from 'toolkit/chakra/color-mode';

export function useChartsConfig(): Array<ChartConfig> {
  const lineColor = useDefaultLineColor();
  const gradient = useDefaultGradient();
  const isMobile = useIsMobile();

  return React.useMemo(() => [
    {
      type: 'line',
      color: lineColor,
      strokeWidth: isMobile ? 1 : 2,
    },
    {
      type: 'area',
      gradient,
    },
  ], [ lineColor, isMobile, gradient ]);
}

export function useDefaultLineColor() {
  const [ lineColor ] = useToken('colors', useColorModeValue('theme.graph.line._light', 'theme.graph.line._dark'));
  return React.useMemo(() => lineColor, [ lineColor ]);
}

export function useDefaultGradient() {
  const [ startColor ] = useToken('colors', useColorModeValue('theme.graph.gradient.start._light', 'theme.graph.gradient.start._dark'));
  const [ stopColor ] = useToken('colors', useColorModeValue('theme.graph.gradient.stop._light', 'theme.graph.gradient.stop._dark'));
  return React.useMemo(() => ({ startColor, stopColor }), [ startColor, stopColor ]);
}
