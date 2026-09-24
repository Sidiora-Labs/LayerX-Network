import type { Feature } from './types';

import apis from '../apis';
import app from '../app';
import account from './account';
import blockchainInteraction from './blockchainInteraction';

const title = 'Rewards service integration';

const config: Feature<{}> = (() => {
  // @0xdeval: as of now, we won't support rewards programs with dynamic auth provider
  if (!app.isPrivateMode && apis.rewards && account.isEnabled && account.authProvider === 'auth0' && blockchainInteraction.isEnabled) {
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
