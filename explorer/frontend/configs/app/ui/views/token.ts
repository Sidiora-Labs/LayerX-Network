import { getEnvValue } from 'configs/app/utils';

const config = Object.freeze({
  hideScamTokensEnabled: getEnvValue('NEXT_PUBLIC_VIEWS_TOKEN_SCAM_TOGGLE_ENABLED') === 'true',
});

export default config;
