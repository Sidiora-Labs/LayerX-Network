import type { Feature } from './types';

import apis from '../apis';

const title = 'Verified tokens info';

const config: Feature<{}> = (() => {
  if (apis.contractInfo) {
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
