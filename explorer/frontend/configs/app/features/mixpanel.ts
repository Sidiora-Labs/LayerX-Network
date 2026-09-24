import type { Config } from 'mixpanel-browser';

import type { Feature } from './types';

import app from '../app';
import { getEnvValue, parseEnvJson } from '../utils';

const projectToken = getEnvValue('NEXT_PUBLIC_MIXPANEL_PROJECT_TOKEN');
const configOverrides = (() => {
  const value = getEnvValue('NEXT_PUBLIC_MIXPANEL_CONFIG_OVERRIDES');
  if (!value) {
    return;
  }

  return parseEnvJson<Partial<Config>>(value) || undefined;
})();

const title = 'Mixpanel analytics';

const config: Feature<{ projectToken: string; configOverrides?: Partial<Config> }> = (() => {
  if (!app.isPrivateMode && projectToken) {
    return Object.freeze({
      title,
      isEnabled: true,
      projectToken,
      configOverrides,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
