import type { Feature } from './types';
import type { GasRefuelProviderConfig } from 'types/client/gasRefuelProviderConfig';

import chain from '../chain';
import { getEnvValue, parseEnvJson } from '../utils';
import marketplace from './marketplace';

const value = parseEnvJson<GasRefuelProviderConfig>(getEnvValue('NEXT_PUBLIC_GAS_REFUEL_PROVIDER_CONFIG'));

const title = 'Get gas button';

const config: Feature<{
  name: string;
  logoUrl?: string;
  url: string;
  dappId?: string;
}> = (() => {
  if (value) {
    return Object.freeze({
      title,
      isEnabled: true,
      name: value.name,
      logoUrl: value.logo,
      url: value.url_template.replace('{chainId}', chain.id || ''),
      dappId: marketplace.isEnabled ? value.dapp_id : undefined,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
