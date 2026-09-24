import type CspDev from 'csp-dev';

import * as multichainConfig from 'configs/multichain/config.edge';

export function multichain(): CspDev.DirectiveDescriptor {
  const value = multichainConfig.getValue();
  if (!value) {
    return {};
  }

  const apiEndpoints = value.chains.map((chain) => {
    return chain.app_config?.apis ? [
      ...Object.values(chain.app_config.apis).filter(Boolean).map((api) => api.endpoint),
      ...Object.values(chain.app_config.apis).filter(Boolean).map((api) => api.socketEndpoint),
    ].filter(Boolean) : [];
  }).flat();

  const rpcEndpoints = value.chains.map(({ app_config: config }) => config?.chain?.rpcUrls).flat().filter(Boolean);

  return {
    'connect-src': [
      ...apiEndpoints,
      ...rpcEndpoints,
      // please see comment in the useFetchParentChainApi.tsx file
      'https://eth.blockscout.com',
    ],
  };
}
