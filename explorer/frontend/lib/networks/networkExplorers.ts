import { mapValues } from 'es-toolkit';

import type { NetworkExplorer } from 'types/networks';

import config from 'configs/app';

// for easy .env update
// const NETWORK_EXPLORERS = JSON.stringify([
//   {
//     title: 'Anyblock',
//     baseUrl: 'https://explorer.anyblock.tools',
//     paths: {
//       tx: '/ethereum/ethereum/goerli/transaction',
//       address: '/ethereum/ethereum/goerli/address'
//     },
//   },
//   {
//     title: 'Etherscan',
//     baseUrl: 'https://goerli.etherscan.io/',
//     paths: {
//       tx: '/tx',
//       address: '/address',
//     },
//   },
// ]).replaceAll('"', '\'');

const stripTrailingSlash = (str: string) => str[str.length - 1] === '/' ? str.slice(0, -1) : str;
const addLeadingSlash = (str: string) => str[0] === '/' ? str : '/' + str;

const networkExplorers: Array<NetworkExplorer> = (() => {
  return config.UI.explorers.items.map((explorer) => ({
    ...explorer,
    baseUrl: stripTrailingSlash(explorer.baseUrl),
    paths: mapValues(explorer.paths, (value) => value ? stripTrailingSlash(addLeadingSlash(value)) : value),
  }));
})();

export default networkExplorers;
