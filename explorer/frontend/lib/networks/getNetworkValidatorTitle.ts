import config from 'configs/app';

export default function getNetworkValidatorTitle() {
  switch (config.chain.verificationType) {
    case 'validation': {
      return 'validator';
    }
    case 'mining': {
      return 'miner';
    }
    case 'posting': {
      return 'poster';
    }
    case 'sequencing': {
      return 'sequencer';
    }
    case 'fee reception': {
      return 'fee recipient';
    }
    default: {
      return 'miner';
    }
  }
}
