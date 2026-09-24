import type { Feature } from './types';

import apis from '../apis';
import multichain from './multichain';

const title = 'Blockchain statistics';

const config: Feature<{}> = (() => {
  if (apis.stats || multichain.isEnabled) {
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
