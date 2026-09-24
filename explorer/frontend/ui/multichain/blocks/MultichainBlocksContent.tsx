import React from 'react';

import multichainConfig from 'configs/multichain';
import getSocketUrl from 'lib/api/getSocketUrl';
import { MultichainProvider } from 'lib/contexts/multichain';
import { SocketProvider } from 'lib/socket/context';
import type { Props as BlocksContentProps } from 'ui/blocks/BlocksContent';
import BlocksContent from 'ui/blocks/BlocksContent';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';

interface Props extends BlocksContentProps {
  chainId: string | undefined;
}

const MultichainBlocksContent = ({ chainId, ...rest }: Props) => {
  const chainConfig = multichainConfig()?.chains.find(chain => chain.id === chainId);

  return (
    <MultichainProvider chainId={ chainId }>
      <SocketProvider url={ chainConfig ? getSocketUrl(chainConfig.app_config) : undefined }>
        <BlocksContent
          { ...rest }
          top={ ACTION_BAR_HEIGHT_DESKTOP }
        />
      </SocketProvider>
    </MultichainProvider>
  );
};

export default React.memo(MultichainBlocksContent);
