import type { Feature } from './types';
import type { ParentChain, RollupType } from 'types/client/rollup';
import { ROLLUP_TYPES } from 'types/client/rollup';

import { stripTrailingSlash } from 'toolkit/utils/url';

import { getEnvValue, parseEnvJson } from '../utils';

const type = (() => {
  const envValue = getEnvValue('NEXT_PUBLIC_ROLLUP_TYPE');
  return ROLLUP_TYPES.find((type) => type === envValue);
})();

const L2WithdrawalUrl = getEnvValue('NEXT_PUBLIC_ROLLUP_L2_WITHDRAWAL_URL');

const parentChain: ParentChain | undefined = (() => {
  const envValue = parseEnvJson<ParentChain>(getEnvValue('NEXT_PUBLIC_ROLLUP_PARENT_CHAIN'));
  if (!envValue?.baseUrl) {
    return;
  }

  return {
    ...envValue,
    baseUrl: stripTrailingSlash(envValue.baseUrl),
  };
})();

const title = 'Rollup (L2) chain';

const config: Feature<{
  type: RollupType;
  stageIndex: string | undefined;
  layerNumber: number;
  homepage: { showLatestBlocks: boolean };
  outputRootsEnabled: boolean;
  interopEnabled: boolean;
  L2WithdrawalUrl: string | undefined;
  parentChain: ParentChain;
  DA: {
    celestia: {
      namespace: string | undefined;
      celeniumUrl: string | undefined;
    };
  };
}> = (() => {
  if (type && parentChain) {
    return Object.freeze({
      title,
      isEnabled: true,
      type,
      stageIndex: getEnvValue('NEXT_PUBLIC_ROLLUP_STAGE_INDEX'),
      layerNumber: Number(getEnvValue('NEXT_PUBLIC_ROLLUP_LAYER_NUMBER') || 2),
      L2WithdrawalUrl: type === 'optimistic' ? L2WithdrawalUrl : undefined,
      outputRootsEnabled: type === 'optimistic' && getEnvValue('NEXT_PUBLIC_ROLLUP_OUTPUT_ROOTS_ENABLED') === 'true',
      interopEnabled: type === 'optimistic' && getEnvValue('NEXT_PUBLIC_INTEROP_ENABLED') === 'true',
      homepage: {
        showLatestBlocks: getEnvValue('NEXT_PUBLIC_ROLLUP_HOMEPAGE_SHOW_LATEST_BLOCKS') === 'true',
      },
      parentChain,
      DA: {
        celestia: {
          namespace: type === 'arbitrum' ? getEnvValue('NEXT_PUBLIC_ROLLUP_DA_CELESTIA_NAMESPACE') : undefined,
          celeniumUrl: getEnvValue('NEXT_PUBLIC_ROLLUP_DA_CELESTIA_CELENIUM_URL'),
        },
      },
    });
  }

  return Object.freeze({
    title,
    isEnabled: false,
  });
})();

export default config;
