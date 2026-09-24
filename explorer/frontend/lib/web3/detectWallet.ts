import type { WalletType } from 'types/client/wallets';
import type { WalletProvider } from 'types/web3';

const isWalletProvider = (wallet: WalletType) => (provider: WalletProvider): boolean | undefined => {
  switch (wallet) {
    case 'rabby': {
      return provider.isRabby;
    }
    case 'coinbase': {
      return provider.isCoinbaseWallet;
    }
    case 'token_pocket': {
      return provider.isTokenPocket;
    }
    case 'okx': {
      return provider.isOkxWallet;
    }
    case 'trust': {
      return provider.isTrustWallet;
    }
    case 'metamask': {
      // some wallets (e.g TokenPocket, Liquality, etc) try to look like MetaMask but they are not (not even close)
      // found this hack in wagmi repo
      // https://github.com/wevm/wagmi/blob/8c35b7adccb4f92c4e0e36a76b970e9126053772/packages/core/src/connectors/injected.ts#L553
      if (!provider.isMetaMask) {
        return false;
      }
      if (!provider._events || !provider._state) {
        return false;
      }
      // Other wallets that try to look like MetaMask
      const flags = [
        'isApexWallet',
        'isAvalanche',
        'isBitKeep',
        'isBlockWallet',
        'isKuCoinWallet',
        'isMathWallet',
        'isOkxWallet',
        'isOKExWallet',
        'isOneInchIOSWallet',
        'isOneInchAndroidWallet',
        'isOpera',
        'isPhantom',
        'isPortal',
        'isRabby',
        'isRainbow',
        'isTokenPocket',
        'isTokenary',
        'isUniswapWallet',
        'isZerion',
      ] ;
      for (const flag of flags) {
        if (provider[flag as keyof WalletProvider]) {
          return false;
        }
      }
      return true;
    }
    default:
      return false;
  }
};

export default function detectWallet(wallet: WalletType): { wallet: WalletType; provider: WalletProvider } | undefined {
  if (!('ethereum' in window && window.ethereum)) {
    return;
  }

  // if user has multiple wallets installed, they all are injected in the window.ethereum.providers array
  // if user has only one wallet, the provider is injected in the window.ethereum directly
  const providers = Array.isArray(window.ethereum.providers) ? window.ethereum.providers : [ window.ethereum ];
  const provider = providers.find(isWalletProvider(wallet));

  if (provider) {
    return { wallet, provider };
  }
}
