import React from 'react';

import multichainConfig from 'configs/multichain';
import type { Props } from 'ui/shared/externalChains/ChainSelect';
import ChainSelect from 'ui/shared/externalChains/ChainSelect';

const ChainSelectMultichain = (props: Omit<Props, 'chainsConfig'>) => {
  const chainsConfig = React.useMemo(() => {
    return multichainConfig()?.chains || [];
  }, []);
  return <ChainSelect chainsConfig={ chainsConfig } { ...props }/>;
};

export default React.memo(ChainSelectMultichain);
