import type { Feature } from './types';
import type { Provider } from 'types/client/txInterpretation';
import { PROVIDERS } from 'types/client/txInterpretation';

import { getEnvValue } from '../utils';

const title = 'Transaction interpretation';

const provider: Provider = (() => {
  const value = getEnvValue('NEXT_PUBLIC_TRANSACTION_INTERPRETATION_PROVIDER');

  if (value && PROVIDERS.includes(value as Provider)) {
    return value as Provider;
  }

  return 'none';
})();

const config: Feature<{ provider: Provider }> = (() => {
  if (provider !== 'none') {
    return Object.freeze({
      title,
      provider,
      isEnabled: true,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
