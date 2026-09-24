import type { Feature } from './types';

import apis from '../apis';
import { getEnvValue } from '../utils';

const title = 'Address metadata';

const config: Feature<{ isAddressTagsUpdateEnabled: boolean }> = (() => {
  if (apis.metadata) {
    return Object.freeze({
      title,
      isEnabled: true,
      isAddressTagsUpdateEnabled: getEnvValue('NEXT_PUBLIC_METADATA_ADDRESS_TAGS_UPDATE_ENABLED') !== 'false',
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
