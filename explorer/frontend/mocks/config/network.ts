import type { FeaturedNetwork } from 'types/networks';

export const FEATURED_NETWORKS: Array<FeaturedNetwork> = [
  { title: 'Gnosis Chain', url: 'https://blockscout.com/xdai/mainnet', group: 'Mainnets', isActive: true },
  { title: 'Arbitrum on xDai', url: 'https://blockscout.com/xdai/aox', group: 'Mainnets' },
  { title: 'Ethereum Classic', url: 'https://blockscout.com/etx/mainnet', group: 'Mainnets', icon: 'https://localhost:3000/my-logo.png' },
  { title: 'Gnosis Chain Testnet', url: 'https://blockscout.com/xdai/testnet', group: 'Testnets' },
  { title: 'POA Sokol', url: 'https://blockscout.com/poa/sokol', group: 'Testnets' },
  { title: 'ARTIS Σ1', url: 'https://blockscout.com/artis/sigma1', group: 'Other' },
  { title: 'LUKSO L14', url: 'https://blockscout.com/lukso/l14', group: 'Other' },
];
