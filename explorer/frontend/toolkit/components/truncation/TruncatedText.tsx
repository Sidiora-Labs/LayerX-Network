// Arbitrary text that will be truncated if there is not enough space in the container

import type { Placement } from '@floating-ui/dom';
import React from 'react';

import type { SkeletonTextProps } from '../../chakra/skeleton';
import { Skeleton } from '../../chakra/skeleton';
import { Tooltip } from '../../chakra/tooltip';
import { TruncatedTextTooltip } from './TruncatedTextTooltip';

export interface TruncatedTextProps extends Omit<SkeletonTextProps, 'loading'> {
  text: string;
  loading?: boolean;
  // tooltipContent is used to display the tooltip value different from the truncated value
  tooltipContent?: string;
  tooltipPlacement?: Placement;
  tooltipInteractive?: boolean;
}

export const TruncatedText = ({ text, tooltipPlacement, tooltipInteractive, tooltipContent, loading, ...rest }: TruncatedTextProps) => {
  const valueElement = (
    <Skeleton
      loading={ loading }
      display="inline-block"
      whiteSpace="nowrap"
      overflow="hidden"
      textOverflow="ellipsis"
      height="fit-content"
      { ...rest }
    >
      <span>{ text }</span>
    </Skeleton>
  );

  // if tooltipContent is provided, we display the tooltip content no matter if the value is truncated or not
  if (tooltipContent) {
    return (
      <Tooltip
        content={ tooltipContent }
        positioning={{ placement: tooltipPlacement }}
        interactive={ tooltipInteractive }
      >
        { valueElement }
      </Tooltip>
    );
  };

  return (
    <TruncatedTextTooltip label={ text } placement={ tooltipPlacement } interactive={ tooltipInteractive }>
      { valueElement }
    </TruncatedTextTooltip>
  );
};
