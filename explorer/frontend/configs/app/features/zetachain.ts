import type { Feature } from './types';

import apis from '../apis';
import { getEnvValue, getExternalAssetFilePath, parseEnvJson } from '../utils';

const title = 'ZetaChain transactions';

const chainsConfigUrl = getExternalAssetFilePath('NEXT_PUBLIC_ZETACHAIN_SERVICE_CHAINS_CONFIG_URL');
const externalSearchConfig = parseEnvJson<Array<{ regex: string; template: string; name: string }>>(
  getEnvValue('NEXT_PUBLIC_ZETACHAIN_EXTERNAL_SEARCH_CONFIG'),
);

const config: Feature<{ chainsConfigUrl: string; externalSearchConfig: Array<{ regex: string; template: string; name: string }> }> = (() => {
  if (apis.zetachain && chainsConfigUrl && externalSearchConfig) {
    return Object.freeze({
      title,
      isEnabled: true,
      chainsConfigUrl,
      externalSearchConfig,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
