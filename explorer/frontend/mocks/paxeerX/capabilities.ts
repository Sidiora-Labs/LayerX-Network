import type { PaxeerXCapabilities } from 'types/api/paxeerX';

export const allEnabled: PaxeerXCapabilities = {
  addr: true,
  custody: true,
  anchor: true,
  exchange: true,
  bridge: true,
  launchpad: true,
};

export const addrOnly: PaxeerXCapabilities = {
  addr: true,
  custody: false,
  anchor: false,
  exchange: false,
  bridge: false,
  launchpad: false,
};

export const none: PaxeerXCapabilities = {
  addr: false,
  custody: false,
  anchor: false,
  exchange: false,
  bridge: false,
  launchpad: false,
};
