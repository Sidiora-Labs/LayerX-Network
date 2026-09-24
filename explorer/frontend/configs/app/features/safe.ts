import type { Feature } from './types';

import { getEnvValue } from '../utils';

function getApiUrl(): string | undefined {
  try {
    const envValue = getEnvValue('NEXT_PUBLIC_SAFE_TX_SERVICE_URL');
    return new URL('/api/v1/safes', envValue).toString();
  } catch (error) {
    return;
  }
}

const title = 'Safe address tags';

const config: Feature<{ apiUrl: string }> = (() => {
  const apiUrl = getApiUrl();

  if (apiUrl) {
    return Object.freeze({
      title,
      isEnabled: true,
      apiUrl,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
