import type { Feature } from './types';

import apis from '../apis';
import app from '../app';
import account from './account';
import verifiedTokens from './verifiedTokens';

const title = 'Address verification in "My account"';

const config: Feature<{}> = (() => {
  if (!app.isPrivateMode && account.isEnabled && verifiedTokens.isEnabled && apis.admin) {
    return Object.freeze({
      title: 'Address verification in "My account"',
      isEnabled: true,
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
