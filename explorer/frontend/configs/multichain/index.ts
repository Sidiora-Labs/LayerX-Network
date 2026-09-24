import type { MultichainConfig } from 'types/multichain';

import config from 'configs/app';
import * as multichainConfigNodejs from 'configs/multichain/config.nodejs';
import { isBrowser } from 'toolkit/utils/isBrowser';

const multichainConfig: () => MultichainConfig | undefined = () => {
  if (!config.features.multichain.isEnabled) {
    return;
  }

  if (isBrowser()) {
    return window.__multichainConfig;
  }

  return multichainConfigNodejs.getValue();
};

export default multichainConfig;
