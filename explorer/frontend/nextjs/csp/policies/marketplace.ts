import type CspDev from 'csp-dev';

import config from 'configs/app';
import * as essentialDappsChains from 'configs/essential-dapps-chains/config.edge';

const feature = config.features.marketplace;

export function marketplace(): CspDev.DirectiveDescriptor {
  if (!feature.isEnabled) {
    return {};
  }

  const chainsConfig = feature.essentialDapps && essentialDappsChains.getValue();
  const externalApiEndpoints = chainsConfig?.chains.map((chain) => chain.app_config?.apis?.general?.endpoint).filter(Boolean);
  const defaultRpcUrls = chainsConfig?.chains.map((chain) => chain.app_config?.chain?.rpcUrls).flat().filter(Boolean);

  const liFiHost = feature.essentialDapps?.swap ? 'li.quest' : '';
  const multisenderHost = feature.essentialDapps?.multisend ? '*.multisender.app' : '';
  const posthogHost = feature.essentialDapps?.multisend?.posthogHost ? '*.posthog.com' : '';

  return {
    'connect-src': [
      'api' in feature ? feature.api.endpoint : '',
      ...(feature.essentialDapps ? [
        liFiHost,
        multisenderHost,
        posthogHost,
        ...(externalApiEndpoints ?? []),
        ...(defaultRpcUrls ?? []),
      ] : []),
    ],

    'frame-src': [
      '*',
    ],

    'script-src': [
      posthogHost,
    ],
  };
}
