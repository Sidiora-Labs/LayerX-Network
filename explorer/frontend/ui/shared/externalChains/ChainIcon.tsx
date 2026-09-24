import React from 'react';

import type { ExternalChain } from 'types/externalChains';

import type { ImageProps } from 'toolkit/chakra/image';
import { Image } from 'toolkit/chakra/image';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';
import IconSvg from 'ui/shared/IconSvg';

import getChainTooltipText from './getChainTooltipText';

interface Props extends ImageProps {
  data: Omit<ExternalChain, 'explorer_url'> | undefined;
  isLoading?: boolean;
  noTooltip?: boolean;
}

const ChainIcon = ({ data, boxSize = 5, borderRadius = 'none', isLoading, noTooltip, ...rest }: Props) => {
  if (isLoading) {
    return <Skeleton boxSize={ boxSize } borderRadius={ borderRadius === 'none' ? 'full' : borderRadius } { ...rest } loading/>;
  }

  const placeholder = <IconSvg name="networks/icon-placeholder" boxSize={ boxSize } color="icon.primary"/>;
  const content = (
    <Image
      src={ data?.logo }
      boxSize={ boxSize }
      borderRadius={ borderRadius }
      fallback={ placeholder }
      alt={ `${ data?.name || 'Unknown' } chain icon` }
      { ...rest }
    />
  );

  if (!noTooltip) {
    return (
      <Tooltip content={ getChainTooltipText(data) }>
        { content }
      </Tooltip>
    );
  }

  return content;
};

export default React.memo(ChainIcon);
