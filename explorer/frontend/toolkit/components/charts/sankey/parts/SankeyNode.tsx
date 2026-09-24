import React from 'react';

import type { SankeyNodeExtended } from '../types';

const OUTER_CORNER_RADIUS = 4;

export interface SankeyNodeProps {
  node: SankeyNodeExtended;
  color: string;
  onMouseEnter?: (node: SankeyNodeExtended, event: React.MouseEvent) => void;
  onMouseLeave?: () => void;
}

export const SankeyNode = React.memo(({ node, color, onMouseEnter, onMouseLeave }: SankeyNodeProps) => {
  const { x0 = 0, x1 = 0, y0 = 0, y1 = 0 } = node;
  const width = x1 - x0;
  const height = y1 - y0;

  const handleMouseEnter = React.useCallback((event: React.MouseEvent) => {
    onMouseEnter?.(node, event);
  }, [ node, onMouseEnter ]);

  const hasIncoming = node.targetLinks?.length;
  const hasOutgoing = node.sourceLinks?.length;
  const isMiddle = Boolean(hasIncoming && hasOutgoing);

  const path = React.useMemo(() => {
    if (width <= 0 || height <= 0 || isMiddle) {
      return null;
    }
    const r = Math.min(OUTER_CORNER_RADIUS, width / 2, height / 2);

    if (hasIncoming) {
      return (
        `M ${ x0 },${ y0 } L ${ x1 - r },${ y0 } Q ${ x1 },${ y0 } ${ x1 },${ y0 + r } ` +
        `L ${ x1 },${ y1 - r } Q ${ x1 },${ y1 } ${ x1 - r },${ y1 } L ${ x0 },${ y1 } L ${ x0 },${ y0 } Z`
      );
    }
    return (
      `M ${ x1 },${ y0 } L ${ x0 + r },${ y0 } Q ${ x0 },${ y0 } ${ x0 },${ y0 + r } ` +
      `L ${ x0 },${ y1 - r } Q ${ x0 },${ y1 } ${ x0 + r },${ y1 } L ${ x1 },${ y1 } L ${ x1 },${ y0 } Z`
    );
  }, [ x0, x1, y0, y1, width, height, isMiddle, hasIncoming ]);

  if (!path) {
    if (isMiddle) {
      return (
        <rect
          x={ x0 }
          y={ y0 }
          width={ width }
          height={ height }
          fill={ color }
          onMouseEnter={ handleMouseEnter }
          onMouseLeave={ onMouseLeave }
        >
          <title>{ `${ node.name }: ${ node.value }` }</title>
        </rect>
      );
    }
    return null;
  }

  return (
    <path
      d={ path }
      fill={ color }
      onMouseEnter={ handleMouseEnter }
      onMouseLeave={ onMouseLeave }
    >
      <title>{ `${ node.name }: ${ node.value }` }</title>
    </path>
  );
});
