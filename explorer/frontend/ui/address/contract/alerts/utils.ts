import type { SmartContractProxyType } from 'types/api/contract';

export const PROXY_TYPES: Partial<Record<NonNullable<SmartContractProxyType>, {
  name: string;
  link?: string;
  description?: string;
}>> = {
  eip1167: {
    name: 'EIP-1167',
    link: 'https://eips.ethereum.org/EIPS/eip-1167',
    description: 'Minimal Proxy',
  },
  eip1967: {
    name: 'EIP-1967',
    link: 'https://eips.ethereum.org/EIPS/eip-1967',
    description: 'Transparent Proxy',
  },
  eip1967_oz: {
    name: 'EIP-1967 (Legacy)',
    link: 'https://eips.ethereum.org/EIPS/eip-1967',
    description: 'Legacy OpenZeppelin Transparent Proxy',
  },
  eip1967_beacon: {
    name: 'EIP-1967 (Beacon)',
    link: 'https://eips.ethereum.org/EIPS/eip-1967',
    description: 'Beacon Proxy',
  },
  eip1822: {
    name: 'EIP-1822',
    link: 'https://eips.ethereum.org/EIPS/eip-1822',
    description: 'Universal Upgradeable Proxy Standard (UUPS)',
  },
  eip2535: {
    name: 'EIP-2535',
    link: 'https://eips.ethereum.org/EIPS/eip-2535',
    description: 'Diamond Proxy',
  },
  erc7760: {
    name: 'ERC-7760',
    link: 'https://eips.ethereum.org/EIPS/eip-7760',
    description: 'Minimal Upgradeable Proxy',
  },
  resolved_delegate_proxy: {
    name: 'ResolvedDelegateProxy',
    // eslint-disable-next-line max-len
    link: 'https://github.com/ethereum-optimism/optimism/blob/9580179013a04b15e6213ae8aa8d43c3f559ed9a/packages/contracts-bedrock/src/legacy/ResolvedDelegateProxy.sol',
    description: 'OP Stack Proxy',
  },
  clone_with_immutable_arguments: {
    name: 'Clones with immutable arguments',
    link: 'https://github.com/wighawag/clones-with-immutable-args',
  },
  master_copy: {
    name: 'Safe Proxy',
    link: 'https://github.com/safe-global/safe-smart-account',
  },
  comptroller: {
    name: 'Compound Protocol Proxy',
    link: 'https://github.com/compound-finance/compound-protocol',
  },
  basic_implementation: {
    name: 'Generic Proxy',
    description: 'implementation() getter',
  },
  basic_get_implementation: {
    name: 'Generic Proxy',
    description: 'getImplementation() getter',
  },
  unknown: {
    name: 'Unknown proxy pattern',
  },
};
