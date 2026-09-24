import { getAddress } from 'viem';

import config from 'configs/app';

const ERC1191_CHAIN_IDS = [
  '30', // RSK Mainnet
  '31', // RSK Testnet
];

export default function getCheckedSummedAddress(address: string): string {
  try {
    return getAddress(
      address,
      // We need to pass chainId to getAddress to make it work correctly for chains that support ERC-1191
      // https://eips.ethereum.org/EIPS/eip-1191#usage--table
      ERC1191_CHAIN_IDS.includes(config.chain.id ?? '') ? Number(config.chain.id) : undefined,
    );
  } catch (error) {
    return address;
  }
}
