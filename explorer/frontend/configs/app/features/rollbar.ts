import type { Feature } from './types';

import app from '../app';
import { getEnvValue } from '../utils';

const clientToken = getEnvValue('NEXT_PUBLIC_ROLLBAR_CLIENT_TOKEN');
const instance = (() => {
  const envValue = getEnvValue('NEXT_PUBLIC_APP_INSTANCE');
  if (envValue) {
    return envValue;
  }

  return app.host?.replace('.blockscout.com', '').replace('.k8s-dev', '').replaceAll('-', '_');
})();
const environment = getEnvValue('NEXT_PUBLIC_APP_ENV') || 'production';
const codeVersion = getEnvValue('NEXT_PUBLIC_GIT_TAG') || getEnvValue('NEXT_PUBLIC_GIT_COMMIT_SHA');

const title = 'Rollbar error monitoring';

const config: Feature<{
  clientToken: string;
  environment: string;
  instance: string | undefined;
  codeVersion: string | undefined;
}> = (() => {
  if (!app.isPrivateMode && clientToken) {
    return Object.freeze({
      title,
      isEnabled: true,
      clientToken,
      environment,
      instance,
      codeVersion,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
