import type { Feature } from './types';

import app from '../app';
import chain from '../chain';
import { getEnvValue, parseEnvJson } from '../utils';
import accountFeature from './account';
import multichain from './multichain';

const walletConnectProjectId = getEnvValue('NEXT_PUBLIC_WALLET_CONNECT_PROJECT_ID');

const title = 'Blockchain interaction (writing to contract, etc.)';

type FeaturePayload = {
  connectorType: 'reown';
  reown: { projectId: string; featuredWalletIds: Array<string> };
} | {
  connectorType: 'dynamic';
  dynamic: { environmentId: string };
};

const config: Feature<FeaturePayload> = (() => {

  // all chain parameters are required for wagmi provider
  // @wagmi/chains/dist/index.d.ts
  const isSingleChain = Boolean(
    chain.id &&
    chain.name &&
    chain.currency.name &&
    chain.currency.symbol &&
    chain.currency.decimals &&
    chain.rpcUrls.length > 0,
  );

  const isMultichain = multichain.isEnabled;

  if (
    !app.isPrivateMode &&
    (isSingleChain || isMultichain)
  ) {
    if (accountFeature.isEnabled && accountFeature.authProvider === 'dynamic' && accountFeature.dynamic?.environmentId) {
      return Object.freeze({
        title,
        isEnabled: true,
        connectorType: 'dynamic',
        dynamic: {
          environmentId: accountFeature.dynamic.environmentId,
        },
      });
    } else if (walletConnectProjectId) {
      return Object.freeze({
        title,
        isEnabled: true,
        connectorType: 'reown',
        reown: {
          projectId: walletConnectProjectId,
          featuredWalletIds: parseEnvJson<Array<string>>(getEnvValue('NEXT_PUBLIC_WALLET_CONNECT_FEATURED_WALLET_IDS')) ?? [],
        },
      });
    }
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
