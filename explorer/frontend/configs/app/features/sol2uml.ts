import type { Feature } from './types';

import apis from '../apis';

const title = 'Solidity to UML diagrams';

const config: Feature<{}> = (() => {
  if (apis.visualize) {
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
