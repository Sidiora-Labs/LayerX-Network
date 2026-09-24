import config from 'configs/app';
import * as essentialDappsChainsConfig from 'configs/essential-dapps-chains/config.nodejs';
import * as multichainConfig from 'configs/multichain/config.nodejs';

const marketplaceFeature = config.features.marketplace;

(async() => {
  config.features.multichain.isEnabled && await multichainConfig.load();
  marketplaceFeature.isEnabled && marketplaceFeature.essentialDapps && await essentialDappsChainsConfig.load();
})();
