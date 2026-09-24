import type { ApiName, ApiResource } from './types';
import type { ExternalChainExtended } from 'types/externalChains';

import config from 'configs/app';

import type { ResourceName } from './resources';
import { RESOURCES } from './resources';

export default function getResourceParams(resourceFullName: ResourceName, chain?: ExternalChainExtended) {
  const [ apiName, resourceName ] = resourceFullName.split(':') as [ ApiName, string ];

  const apiConfig = (() => {
    if (chain?.app_config?.apis) {
      return chain.app_config.apis[apiName as keyof typeof chain.app_config.apis];
    }

    return config.apis[apiName];
  })();

  if (!apiConfig) {
    throw new Error(`API config for ${ apiName } not found`);
  }

  return {
    api: apiConfig,
    apiName,
    resource: RESOURCES[apiName][resourceName as keyof typeof RESOURCES[ApiName]] as ApiResource,
  };
}
