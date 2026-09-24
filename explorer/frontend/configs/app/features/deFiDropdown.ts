import type { Feature } from './types';
import type { DeFiDropdownItem } from 'types/client/deFiDropdown';

import app from '../app';
import { getEnvValue, parseEnvJson } from '../utils';

const items = parseEnvJson<Array<DeFiDropdownItem>>(getEnvValue('NEXT_PUBLIC_DEFI_DROPDOWN_ITEMS')) || [];

const title = 'DeFi dropdown';

const config: Feature<{ items: Array<DeFiDropdownItem> }> = (() => {
  if (!app.isPrivateMode && items.length > 0) {
    return Object.freeze({
      title,
      isEnabled: true,
      items,
    });
  }
  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
