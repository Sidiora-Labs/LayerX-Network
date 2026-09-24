import type { WalletType, WalletInfo } from 'types/client/wallets';

export const WALLETS_INFO: Record<Exclude<WalletType, 'none'>, WalletInfo> = {
  metamask: {
    name: 'MetaMask',
    icon: 'wallets/metamask',
    color: '#FF8D5D',
  },
  coinbase: {
    name: 'Coinbase Wallet',
    icon: 'wallets/coinbase',
    color: '#0052FF',
  },
  token_pocket: {
    name: 'TokenPocket',
    icon: 'wallets/token-pocket',
    color: '#2980FE',
  },
  rabby: {
    name: 'Rabby',
    icon: 'wallets/rabby',
    color: '#8798FF',
  },
  okx: {
    name: 'OKX Wallet',
    icon: 'wallets/okx',
    color: '#6DB809',
  },
  trust: {
    name: 'Trust Wallet',
    icon: 'wallets/trust',
    color: '#2D77FF',
  },
};
