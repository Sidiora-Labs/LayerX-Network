import { select } from 'd3-selection';
import 'd3-transition';
import React from 'react';

import type { SankeyLinkExtended } from '../types';

export interface SankeyLinkProps {
  link: SankeyLinkExtended;

  /** Single color when gradient is not used */
  color?: string;

  /** When set with targetColor, link stroke is a gradient from source to target */
  sourceColor?: string;
  targetColor?: string;

  /** Unique suffix for gradient id (e.g. link index) when multiple links share same source/target */
  gradientIdSuffix?: string | number;
  opacity: number;
  hoverOpacity: number;

  pathGenerator: (link: SankeyLinkExtended, ...args: Array<any>) => string | null;
  onMouseEnter?: (link: SankeyLinkExtended, event: React.MouseEvent) => void;
  onMouseLeave?: () => void;
}

export const SankeyLink = React.memo(({
  link,
  color,
  sourceColor,
  targetColor,
  gradientIdSuffix,
  opacity,
  hoverOpacity,
  pathGenerator,
  onMouseEnter,
  onMouseLeave,
}: SankeyLinkProps) => {
  const ref = React.useRef<SVGPathElement>(null);

  const handleMouseEnter = React.useCallback((event: React.MouseEvent) => {
    if (ref.current) {
      select(ref.current).transition().duration(200).attr('stroke-opacity', hoverOpacity);
    }
    onMouseEnter?.(link, event);
  }, [ link, hoverOpacity, onMouseEnter ]);

  const handleMouseLeave = React.useCallback(() => {
    if (ref.current) {
      select(ref.current).transition().duration(200).attr('stroke-opacity', opacity);
    }
    onMouseLeave?.();
  }, [ opacity, onMouseLeave ]);

  const source = link.source;
  const target = link.target;

  const d = pathGenerator(link);
  if (!d) {
    return null;
  }

  const sanitizeSvgId = (value: string | number): string => String(value).replace(/[^\w-]/g, '_');
  const useGradient = sourceColor != null && targetColor != null && sourceColor !== targetColor;
  const gradientId = useGradient ?
    `sankey-link-${ sanitizeSvgId(source.id) }-${ sanitizeSvgId(target.id) }-${ sanitizeSvgId(gradientIdSuffix ?? '') }` :
    undefined;

  const strokeColor = useGradient && gradientId ?
    `url(#${ gradientId })` :
    color;

  const sourceX = source.x1 ?? 0;
  const sourceY = ((source.y0 ?? 0) + (source.y1 ?? 0)) / 2;
  const targetX = target.x0 ?? 0;
  const targetY = ((target.y0 ?? 0) + (target.y1 ?? 0)) / 2;

  return (
    <>
      { useGradient && gradientId && (
        <defs>
          <linearGradient
            id={ gradientId }
            gradientUnits="userSpaceOnUse"
            x1={ sourceX }
            y1={ sourceY }
            x2={ targetX }
            y2={ targetY }
          >
            <stop offset="0%" stopColor={ sourceColor }/>
            <stop offset="100%" stopColor={ targetColor }/>
          </linearGradient>
        </defs>
      ) }
      <path
        ref={ ref }
        d={ d }
        fill="none"
        stroke={ strokeColor }
        strokeOpacity={ opacity }
        strokeWidth={ Math.max(1, link.width || 0) }
        onMouseEnter={ handleMouseEnter }
        onMouseLeave={ handleMouseLeave }
      >
        <title>{ `${ source.name } → ${ target.name }: ${ link.value }` }</title>
      </path>
    </>
  );
});
