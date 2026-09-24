import mixpanel from 'mixpanel-browser';

import config from 'configs/app';

export default function reset() {
  if (!config.features.mixpanel.isEnabled) {
    return;
  }
  mixpanel.reset();
}
