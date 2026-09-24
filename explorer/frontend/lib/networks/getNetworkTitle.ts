import config from 'configs/app';

// TODO delete when page descriptions is refactored
export default function getNetworkTitle() {
  return config.chain.name + (config.chain.shortName ? ` (${ config.chain.shortName })` : '') + ' Explorer';
}
