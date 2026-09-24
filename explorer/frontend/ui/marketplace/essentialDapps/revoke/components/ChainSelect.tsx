import React from 'react';

import essentialDappsChainsConfig from 'configs/essential-dapps-chains';
import type { Props } from 'ui/shared/externalChains/ChainSelect';
import ChainSelect from 'ui/shared/externalChains/ChainSelect';

const ChainSelectEssentialDapps = (props: Omit<Props, 'chainsConfig'>) => {
  const chainsConfig = React.useMemo(() => {
    return essentialDappsChainsConfig()?.chains || [];
  }, []);
  return <ChainSelect chainsConfig={ chainsConfig } { ...props }/>;
};

export default React.memo(ChainSelectEssentialDapps);
