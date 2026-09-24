import type { Feature } from './types';

import apis from '../apis';
import { getEnvValue, parseEnvJson } from '../utils';

const title = 'Name services integration';

const config: Feature<{ ens: { isEnabled: boolean; protocols: Array<string> }; clusters: { isEnabled: boolean; cdnUrl: string } }> = (() => {
  if (apis.bens || apis.clusters) {
    return Object.freeze({
      title,
      isEnabled: true,
      ens: {
        isEnabled: apis.bens ? true : false,
        protocols: parseEnvJson<Array<string>>(getEnvValue('NEXT_PUBLIC_NAME_SERVICE_PROTOCOLS')) || [ 'ens' ],
      },
      clusters: {
        isEnabled: apis.clusters ? true : false,
        cdnUrl: getEnvValue('NEXT_PUBLIC_CLUSTERS_CDN_URL') || 'https://cdn.clusters.xyz',
      },
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
