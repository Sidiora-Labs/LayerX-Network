import type { GetServerSidePropsContext, GetServerSidePropsResult } from 'next';

import type { RollupType } from 'types/client/rollup';

import type { Route } from 'nextjs-routes';
import type { Props } from 'nextjs/getServerSideProps/handlers';

import config from 'configs/app';

export type Guard = (chainConfig: typeof config) => <Pathname extends Route['pathname'] = never>(context: GetServerSidePropsContext) =>
Promise<GetServerSidePropsResult<Props<Pathname>> | undefined>;

export const account: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.account.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const accountAuth0: Guard = (chainConfig: typeof config) => async() => {
  const feature = chainConfig.features.account;
  if (!feature.isEnabled || feature.authProvider !== 'auth0') {
    return {
      notFound: true,
    };
  }
};

export const verifiedAddresses: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.addressVerification.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const userOps: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.userOps.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const marketplace: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.marketplace.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const marketplaceEssentialDapp: Guard = (chainConfig: typeof config) => async() => {
  const feature = chainConfig.features.marketplace;
  if (!feature.isEnabled || !feature.essentialDapps) {
    return {
      notFound: true,
    };
  }
};

export const apiDocs: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.apiDocs.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const csvExport: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.csvExport.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const stats: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.stats.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const suave: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.suave.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const nameServiceEns: Guard = (chainConfig: typeof config) => async() => {
  const feature = chainConfig.features.nameServices;
  if (!feature.isEnabled || !feature.ens.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const nameServiceClusters: Guard = (chainConfig: typeof config) => async() => {
  const feature = chainConfig.features.nameServices;
  if (!feature.isEnabled || !feature.clusters.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const accounts: Guard = (chainConfig: typeof config) => async() => {
  if (chainConfig.UI.views.address.hiddenViews?.top_accounts) {
    return {
      notFound: true,
    };
  }
};

export const accountsLabelSearch: Guard = (chainConfig: typeof config) => async(context) => {
  if (!chainConfig.features.addressMetadata.isEnabled || !context.query.tagType) {
    return {
      notFound: true,
    };
  }
};

export const validators: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.validators.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const validatorDetails: Guard = (chainConfig: typeof config) => async() => {
  const feature = chainConfig.features.validators;
  if (!feature.isEnabled || feature.chainType !== 'zilliqa') {
    return {
      notFound: true,
    };
  }
};

export const gasTracker: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.gasTracker.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const hotContracts: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.hotContracts.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const advancedFilter: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.advancedFilter.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const dataAvailability: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.dataAvailability.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const login: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.app.isReview && !chainConfig.app.isDev) {
    return {
      notFound: true,
    };
  }
};

export const dev: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.app.isDev) {
    return {
      notFound: true,
    };
  }
};

export const publicTagsSubmit: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.publicTagsSubmission.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const pools: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.pools.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const zetaChainCCTX: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.zetachain.isEnabled) {
    return {
      notFound: true,
    };
  }
};

// ROLLUPS
export const rollup: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.rollup.isEnabled) {
    return {
      notFound: true,
    };
  }
};

const DEPOSITS_ROLLUP_TYPES: Array<RollupType> = [ 'optimistic', 'shibarium', 'zkEvm', 'arbitrum', 'scroll' ];
export const deposits: Guard = (chainConfig: typeof config) => async() => {
  const rollupFeature = chainConfig.features.rollup;
  const beaconChainFeature = chainConfig.features.beaconChain;
  if (
    (!beaconChainFeature.isEnabled || beaconChainFeature.withdrawalsOnly) &&
    !(rollupFeature.isEnabled && DEPOSITS_ROLLUP_TYPES.includes(rollupFeature.type))) {
    return {
      notFound: true,
    };
  }
};

const WITHDRAWALS_ROLLUP_TYPES: Array<RollupType> = [ 'optimistic', 'shibarium', 'zkEvm', 'arbitrum', 'scroll' ];
export const withdrawals: Guard = (chainConfig: typeof config) => async() => {
  const rollupFeature = chainConfig.features.rollup;
  if (
    !chainConfig.features.beaconChain.isEnabled &&
    !(rollupFeature.isEnabled && WITHDRAWALS_ROLLUP_TYPES.includes(rollupFeature.type))
  ) {
    return {
      notFound: true,
    };
  }
};

const BATCH_ROLLUP_TYPES: Array<RollupType> = [ 'zkEvm', 'zkSync', 'arbitrum', 'optimistic', 'scroll' ];
export const batch: Guard = (chainConfig: typeof config) => async() => {
  const rollupFeature = chainConfig.features.rollup;
  if (!(rollupFeature.isEnabled && BATCH_ROLLUP_TYPES.includes(rollupFeature.type))) {
    return {
      notFound: true,
    };
  }
};

export const batchCelestia: Guard = (chainConfig: typeof config) => async() => {
  const rollupFeature = chainConfig.features.rollup;
  if (!(rollupFeature.isEnabled && (rollupFeature.type === 'arbitrum' || rollupFeature.type === 'optimistic'))) {
    return {
      notFound: true,
    };
  }
};

export const txnWithdrawals: Guard = (chainConfig: typeof config) => async() => {
  const rollupFeature = chainConfig.features.rollup;
  if (!(rollupFeature.isEnabled && rollupFeature.type === 'arbitrum')) {
    return {
      notFound: true,
    };
  }
};

export const outputRoots: Guard = (chainConfig: typeof config) => async() => {
  const rollupFeature = chainConfig.features.rollup;
  if (!(rollupFeature.isEnabled && rollupFeature.outputRootsEnabled)) {
    return {
      notFound: true,
    };
  }
};

export const disputeGames: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.faultProofSystem.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const mud: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.mudFramework.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const tac: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.tac.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const celo: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.celo.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const interopMessages: Guard = (chainConfig: typeof config) => async() => {
  const rollupFeature = chainConfig.features.rollup;
  if (!rollupFeature.isEnabled || !rollupFeature.interopEnabled) {
    return {
      notFound: true,
    };
  }
};

export const crossChainTxs: Guard = (chainConfig: typeof config) => async() => {
  if (!chainConfig.features.crossChainTxs.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const multichain: Guard = () => async() => {
  if (!config.features.multichain.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const notMultichain: Guard = () => async() => {
  if (config.features.multichain.isEnabled) {
    return {
      notFound: true,
    };
  }
};

export const megaEth: Guard = () => async() => {
  if (!config.features.megaEth.isEnabled) {
    return {
      notFound: true,
    };
  }
};
