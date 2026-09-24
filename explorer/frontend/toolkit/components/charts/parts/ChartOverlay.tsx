import React from 'react';

export interface ChartOverlayProps {
  width: number;
  height: number;
  children: React.ReactNode;
}

export const ChartOverlay = React.forwardRef(({ width, height, children }: ChartOverlayProps, ref: React.ForwardedRef<SVGRectElement>) => {
  return (
    <g className="ChartOverlay">
      { children }
      <rect ref={ ref } width={ width } height={ height } opacity={ 0 }/>
    </g>
  );
});
