import { chakra } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import { Image } from 'toolkit/chakra/image';

interface Props {
  className?: string;
}

const TestnetBadge = ({ className }: Props) => {
  if (!config.chain.isTestnet) {
    return null;
  }

  return <Image className={ className } src="/static/labels/testnet.svg" h="14px" w="37px" color="red.400"/>;
};

export default React.memo(chakra(TestnetBadge));
