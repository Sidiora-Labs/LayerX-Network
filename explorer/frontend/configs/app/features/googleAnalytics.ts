import type { Feature } from './types';

import app from '../app';
import { getEnvValue } from '../utils';

const propertyId = getEnvValue('NEXT_PUBLIC_GOOGLE_ANALYTICS_PROPERTY_ID');

const title = 'Google analytics';

const config: Feature<{ propertyId: string }> = (() => {
  if (!app.isPrivateMode && propertyId) {
    return Object.freeze({
      title,
      isEnabled: true,
      propertyId,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
