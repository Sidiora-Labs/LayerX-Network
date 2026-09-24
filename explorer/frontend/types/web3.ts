// copied from node_modules/@wagmi/core/src/connectors/injected.ts
import type { EIP1193Provider } from 'viem';

import type { Evaluate } from './utils';

type WalletProviderFlags =
  'isApexWallet' |
  'isAvalanche' |
  'isBackpack' |
  'isBifrost' |
  'isBitKeep' |
  'isBitski' |
  'isBlockWallet' |
  'isBraveWallet' |
  'isCoinbaseWallet' |
  'isDawn' |
  'isEnkrypt' |
  'isExodus' |
  'isFrame' |
  'isFrontier' |
  'isGamestop' |
  'isHyperPay' |
  'isImToken' |
  'isKuCoinWallet' |
  'isMathWallet' |
  'isMetaMask' |
  'isOkxWallet' |
  'isOKExWallet' |
  'isOneInchAndroidWallet' |
  'isOneInchIOSWallet' |
  'isOpera' |
  'isPhantom' |
  'isPortal' |
  'isRabby' |
  'isRainbow' |
  'isStatus' |
  'isTally' |
  'isTokenPocket' |
  'isTokenary' |
  'isTrust' |
  'isTrustWallet' |
  'isXDEFI' |
  'isZerion';

export type WalletProvider = Evaluate<
EIP1193Provider & {
  [key in WalletProviderFlags]?: true | undefined
} & {
  providers?: Array<WalletProvider> | undefined;

  /** Only exists in MetaMask as of 2022/04/03 */
  _events?: { connect?: (() => void) | undefined } | undefined;

  /** Only exists in MetaMask as of 2022/04/03 */
  _state?:
  | {
    accounts?: Array<string>;
    initialized?: boolean;
    isConnected?: boolean;
    isPermanentlyDisconnected?: boolean;
    isUnlocked?: boolean;
  } |
  undefined;
}
>;
