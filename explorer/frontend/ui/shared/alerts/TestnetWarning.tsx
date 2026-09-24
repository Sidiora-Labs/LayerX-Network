import { chakra } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import { Alert } from 'toolkit/chakra/alert';

interface Props {
  isLoading?: boolean;
  className?: string;
}

const TestnetWarning = ({ isLoading, className }: Props) => {
  if (!config.chain.isTestnet) {
    return null;
  }

  return (
    <Alert status="warning" loading={ isLoading } className={ className }>This is a testnet transaction only</Alert>
  );
};

export default React.memo(chakra(TestnetWarning));
