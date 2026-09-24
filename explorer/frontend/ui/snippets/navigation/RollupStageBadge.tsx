import React from 'react';

import config from 'configs/app';
import type { ImageProps } from 'toolkit/chakra/image';
import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';

interface Props extends ImageProps {
  chainConfig?: typeof config;
  isLoading?: boolean;
}

const RollupStageBadge = ({ chainConfig = config, isLoading, ...props }: Props) => {

  const feature = chainConfig.features.rollup;

  if (!feature.isEnabled || chainConfig.chain.isTestnet) {
    return null;
  }

  if (isLoading) {
    return <Skeleton w="42px" h="14px" { ...props } loading/>;
  }

  switch (feature.stageIndex) {
    case '1':
    case '2': {
      const tooltipContent = (
        <>
          The decentralization and maturity of the chain. Learn more at{ ' ' }
          <Link href="https://l2beat.com?utm_source=blockscout&utm_medium=explorer" external>
            l2beat.com
          </Link>
        </>
      );

      return (
        <Tooltip content={ tooltipContent } interactive>
          <Image
            src={ feature.stageIndex === '1' ? '/static/labels/stage-1.svg' : '/static/labels/stage-2.svg' }
            h="14px"
            w="42px"
            { ...props }
          />
        </Tooltip>
      );
    }
    default:
      return null;
  }
};

export default React.memo(RollupStageBadge);
