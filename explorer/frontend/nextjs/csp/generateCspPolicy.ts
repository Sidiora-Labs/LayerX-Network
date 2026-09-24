import * as descriptors from './policies';
import { makePolicyString, mergeDescriptors } from './utils';

function generateCspPolicy(isPrivateMode = false) {
  const policyDescriptor = mergeDescriptors(
    descriptors.app(isPrivateMode),
    // Exclude tracking/analytics sources in private mode
    isPrivateMode ? {} : descriptors.ad(),
    isPrivateMode ? {} : descriptors.blockchainInteraction(),
    descriptors.cloudFlare(),
    descriptors.flashblocks(),
    isPrivateMode ? {} : descriptors.googleAnalytics(),
    descriptors.googleFonts(),
    descriptors.googleReCaptcha(),
    isPrivateMode ? {} : descriptors.growthBook(),
    descriptors.helia(),
    isPrivateMode ? {} : descriptors.marketplace(),
    descriptors.megaEth(),
    isPrivateMode ? {} : descriptors.mixpanel(),
    descriptors.monaco(),
    descriptors.multichain(),
    isPrivateMode ? {} : descriptors.rollbar(),
    descriptors.rollup(),
    descriptors.safe(),
    descriptors.usernameApi(),
    descriptors.zetachain(),
  );

  return makePolicyString(policyDescriptor);
}

export default generateCspPolicy;
