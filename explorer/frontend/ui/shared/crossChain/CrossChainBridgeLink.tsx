import type { JsxStyleProps } from '@chakra-ui/react';
import React from 'react';

import type { BridgeInfo } from '@blockscout/interchain-indexer-types';

import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';

interface Props extends JsxStyleProps {
  data: BridgeInfo | undefined;
  isLoading?: boolean;
}

const CrossChainBridgeLink = ({ data, isLoading, ...rest }: Props) => {

  if (!data) {
    return null;
  }

  if (data.ui_url) {
    return (
      <Link href={ data.ui_url } external loading={ isLoading } { ...rest }>
        { data.name }
      </Link>
    );
  }

  return (
    <Skeleton loading={ isLoading } { ...rest }>{ data.name }</Skeleton>
  );
};

export default React.memo(CrossChainBridgeLink);
