import type { BoxProps } from '@chakra-ui/react';
import { Box, chakra } from '@chakra-ui/react';
import type BigNumber from 'bignumber.js';
import React from 'react';

import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';
import CopyToClipboard from 'ui/shared/CopyToClipboard';

import { DEFAULT_ACCURACY, formatBnValue } from './utils';

const TOOLTIP_CONTENT_PROPS = { maxW: { base: 'calc(100vw - 8px)', lg: '400px' } };

export interface Props extends Omit<BoxProps, 'prefix' | 'postfix'> {
  value: BigNumber;
  accuracy?: number;
  prefix?: string;
  postfix?: string;
  startElement?: React.ReactNode;
  endElement?: React.ReactNode;
  tooltipContent?: React.ReactNode;
  noTooltip?: boolean;
  loading?: boolean;
  overflowed?: boolean;
  tooltipContentBefore?: React.ReactNode;
}

const SimpleValue = ({
  value,
  accuracy = DEFAULT_ACCURACY,
  prefix,
  postfix,
  startElement,
  endElement,
  tooltipContent: tooltipContentProp,
  tooltipContentBefore,
  noTooltip,
  loading,
  overflowed,
  ...rest
}: Props) => {

  const tooltipContent = React.useMemo(() => {
    return (
      <>
        { tooltipContentBefore }
        <Box display="inline" whiteSpace="wrap" wordBreak="break-all">
          { prefix ?? '' }{ value.toFormat() }{ postfix ?? '' }
          <CopyToClipboard text={ value.toFixed() } verticalAlign="bottom" noTooltip/>
        </Box>
      </>
    );
  }, [ postfix, prefix, value, tooltipContentBefore ]);

  return (
    <Skeleton loading={ loading } display="inline-flex" alignItems="center" whiteSpace="pre" maxW="100%" overflow="hidden" { ...rest }>
      { startElement }
      <Tooltip
        content={ tooltipContentProp ?? tooltipContent }
        contentProps={ TOOLTIP_CONTENT_PROPS }
        disabled={ noTooltip }
        interactive
      >
        <chakra.span display="inline-block" maxW="100%" overflow="hidden" textOverflow="ellipsis">
          { formatBnValue({ value, accuracy, prefix, postfix, overflowed }) }
        </chakra.span>
      </Tooltip>
      { typeof endElement === 'string' ? <span>{ endElement }</span> : endElement }
    </Skeleton>
  );
};

export default React.memo(SimpleValue);
