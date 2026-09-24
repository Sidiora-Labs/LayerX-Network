import type { Feature } from './types';

import services from '../services';

const title = 'Export data to CSV file';

const config: Feature<{ reCaptcha: { siteKey: string } }> = (() => {
  if (services.reCaptchaV2.siteKey) {
    return Object.freeze({
      title,
      isEnabled: true,
      reCaptcha: {
        siteKey: services.reCaptchaV2.siteKey,
      },
    });
  }
  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
