import type { Feature } from './types';

import apis from '../apis';
import app from '../app';
import services from '../services';
import addressMetadata from './addressMetadata';

const title = 'Public tag submission';

const config: Feature<{}> = (() => {
  if (!app.isPrivateMode && services.reCaptchaV2.siteKey && addressMetadata.isEnabled && apis.admin) {
    return Object.freeze({
      title,
      isEnabled: true,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
