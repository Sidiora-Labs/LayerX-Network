import type { ChartMargin, ChartOffset } from '../types';

export function calculateInnerSize(rect: DOMRect | null, margin?: ChartMargin, offsets?: ChartOffset) {
  if (!rect) {
    return { innerWidth: 0, innerHeight: 0 };
  }

  return {
    innerWidth: Math.max(rect.width - (offsets?.x || 0) - (margin?.left || 0) - (margin?.right || 0), 0),
    innerHeight: Math.max(rect.height - (offsets?.y || 0) - (margin?.bottom || 0) - (margin?.top || 0), 0),
  };
}
