import type { BadgeProps as ChakraBadgeProps } from '@chakra-ui/react';
import { chakra, Badge as ChakraBadge } from '@chakra-ui/react';
import React from 'react';

import { TruncatedTextTooltip } from '../components/truncation/TruncatedTextTooltip';
import { Skeleton } from './skeleton';

export interface BadgeProps extends Omit<ChakraBadgeProps, 'colorScheme'> {
  loading?: boolean;
  startElement?: React.ReactNode;
  endElement?: React.ReactNode;
  truncated?: boolean;
}

export const Badge = React.forwardRef<HTMLDivElement, BadgeProps>(
  function Badge(props, ref) {
    const { loading, startElement, children, asChild = true, truncated = false, endElement, ...rest } = props;

    const child = children ? <chakra.span overflow="hidden" textOverflow="ellipsis">{ children }</chakra.span> : null;

    const childrenElement = truncated ? (
      <TruncatedTextTooltip label={ children }>
        { child }
      </TruncatedTextTooltip>
    ) : child;

    return (
      <Skeleton loading={ loading } asChild={ asChild } ref={ ref }>
        <ChakraBadge display="inline-flex" alignItems="center" whiteSpace="nowrap" { ...rest }>
          { startElement }
          { childrenElement }
          { endElement }
        </ChakraBadge>
      </Skeleton>
    );
  });
