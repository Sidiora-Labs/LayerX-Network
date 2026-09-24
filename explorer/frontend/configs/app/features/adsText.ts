import type { Feature } from './types';
import { SUPPORTED_AD_TEXT_PROVIDERS } from 'types/client/adProviders';
import type { AdTextProviders } from 'types/client/adProviders';

import app from '../app';
import { getEnvValue } from '../utils';

const SEVIO_ZONE = '179b0a0e-19b0-4b2a-ad15-7446870bf189';
const SEVIO_INVENTORY_ID = '65597ae2-67b8-404b-ac28-43d5029389da';
const SEVIO_ACCOUNT_ID = 'e08c2e1e-2213-49d8-a397-c2e32094fba4';
const SEVIO_AD_TYPE = 'native';

interface SevioConfig {
  readonly zone: string;
  readonly inventoryId: string;
  readonly accountId: string;
  readonly adType: string;
}

const provider: AdTextProviders = (() => {
  const envValue = getEnvValue('NEXT_PUBLIC_AD_TEXT_PROVIDER') as AdTextProviders;
  return envValue && SUPPORTED_AD_TEXT_PROVIDERS.includes(envValue) ? envValue : 'coinzilla';
})();

const title = 'Text ads';

const config: Feature<{ provider: AdTextProviders; sevio: SevioConfig }> = (() => {
  if (!app.isPrivateMode && provider !== 'none') {
    return Object.freeze({
      title,
      isEnabled: true,
      provider,
      sevio: {
        zone: SEVIO_ZONE,
        inventoryId: SEVIO_INVENTORY_ID,
        accountId: SEVIO_ACCOUNT_ID,
        adType: SEVIO_AD_TYPE,
      },
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
