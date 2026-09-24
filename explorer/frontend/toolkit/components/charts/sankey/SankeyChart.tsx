import { useToken } from '@chakra-ui/react';
import React from 'react';

import type { ChartMargin } from '../types';
import type { SankeyData, SankeyNodeExtended } from './types';

import { useColorModeValue } from 'toolkit/chakra/color-mode';

import {
  DEFAULT_SANKEY_LINK_HOVER_OPACITY,
  DEFAULT_SANKEY_LINK_OPACITY,
  SANKEY_NODE_COLOR_TOKENS_DARK,
  SANKEY_NODE_COLOR_TOKENS_LIGHT,
} from './constants';
import { SankeyLink } from './parts/SankeyLink';
import { SankeyNode } from './parts/SankeyNode';
import { useSankeyController } from './useSankeyController';

export type SankeyLinkColorMode = 'source' | 'target';

export interface SankeyChartProps {
  data: SankeyData;
  margin?: ChartMargin;
  nodeWidth?: number;
  nodePadding?: number;
  linkOpacity?: number;
  linkHoverOpacity?: number;
  linkColorMode?: SankeyLinkColorMode;
  colors?: ReadonlyArray<string>;
  valueFormatter?: (value: number) => string;
}

const DEFAULT_CHART_MARGIN: ChartMargin = { top: 40, right: 8, bottom: 8, left: 8 };
const LABEL_Y_GAP = 4;
const LABEL_LINE_HEIGHT = 16;
const LABEL_LINE_GAP = 2;

export const SankeyChart = React.memo(({
  data,
  margin: marginProps,
  nodeWidth,
  nodePadding,
  linkOpacity = DEFAULT_SANKEY_LINK_OPACITY,
  linkHoverOpacity = DEFAULT_SANKEY_LINK_HOVER_OPACITY,
  linkColorMode = 'source',
  colors: colorsProp,
  valueFormatter,
}: SankeyChartProps) => {
  const margin: ChartMargin = React.useMemo(() => ({ ...DEFAULT_CHART_MARGIN, ...marginProps }), [ marginProps ]);

  const colorTokens = useColorModeValue(SANKEY_NODE_COLOR_TOKENS_LIGHT, SANKEY_NODE_COLOR_TOKENS_DARK);
  const resolvedDefaultColors = useToken('colors', colorTokens);
  const colors = colorsProp && colorsProp.length > 0 ? colorsProp : resolvedDefaultColors;

  const [ labelColor ] = useToken('colors', 'text.secondary');
  const [ labelFontSize ] = useToken('fontSizes', 'xs');

  const {
    ref,
    rect,
    innerWidth,
    nodes,
    links,
    linkPathGenerator,
  } = useSankeyController({
    data,
    margin,
    nodeWidth,
    nodePadding,
  });

  const nodeColorMap = React.useMemo(() => {
    const map = new Map<string, string>();
    nodes.forEach((node, index) => {
      map.set(node.id, colors[index % colors.length]);
    });
    return map;
  }, [ nodes, colors ]);

  const getNodeColor = React.useCallback((node: SankeyNodeExtended) => {
    return nodeColorMap.get(node.id) || colors[0];
  }, [ nodeColorMap, colors ]);

  const formatValue = React.useCallback((value: number | undefined) => {
    if (value === undefined) {
      return '';
    }
    return valueFormatter ? valueFormatter(value) : String(value);
  }, [ valueFormatter ]);

  return (
    <svg width="100%" height="100%" ref={ ref } opacity={ rect ? 1 : 0 }>
      <g transform={ `translate(${ margin.left || 0 },${ margin.top || 0 })` }>
        { links.map((link, index) => (
          <SankeyLink
            key={ `link-${ index }` }
            link={ link }
            color={ getNodeColor(linkColorMode === 'target' ? link.target : link.source) }
            sourceColor={ getNodeColor(link.source) }
            targetColor={ getNodeColor(link.target) }
            gradientIdSuffix={ index }
            opacity={ linkOpacity }
            hoverOpacity={ linkHoverOpacity }
            pathGenerator={ linkPathGenerator }
          />
        )) }

        { nodes.map((node) => (
          <SankeyNode
            key={ node.id }
            node={ node }
            color={ getNodeColor(node) }
          />
        )) }

        { nodes.map((node) => {
          const { x0 = 0, x1 = 0, y0 = 0 } = node;
          const isSource = (x0 || 0) <= innerWidth / 2;
          const nodeId = node.id;
          const nodeName = node.name;
          const valueStr = node.value !== undefined ? formatValue(node.value) : '';

          const blockHeight = LABEL_LINE_HEIGHT + (valueStr ? LABEL_LINE_GAP + LABEL_LINE_HEIGHT : 0);
          const blockTop = (y0 || 0) - LABEL_Y_GAP - blockHeight;
          const nameCenterY = blockTop + LABEL_LINE_HEIGHT / 2;
          const valueCenterY = blockTop + LABEL_LINE_HEIGHT + LABEL_LINE_GAP + LABEL_LINE_HEIGHT / 2;

          const labelX = isSource ? (x0 || 0) : (x1 || 0);
          const anchor = isSource ? 'start' : 'end';

          return (
            <g key={ `label-${ nodeId }` }>
              <text
                x={ labelX }
                y={ nameCenterY }
                textAnchor={ anchor }
                dominantBaseline="central"
                fill={ labelColor }
                style={{ fontSize: labelFontSize, fontWeight: 600 }}
              >
                { nodeName }
              </text>
              { valueStr && (
                <text
                  x={ labelX }
                  y={ valueCenterY }
                  textAnchor={ anchor }
                  dominantBaseline="central"
                  fill={ labelColor }
                  style={{ fontSize: labelFontSize, fontWeight: 400 }}
                >
                  { valueStr }
                </text>
              ) }
            </g>
          );
        }) }
      </g>
    </svg>
  );
});
